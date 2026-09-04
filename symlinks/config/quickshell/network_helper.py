#!/usr/bin/env python3
"""NetworkManager queries and explicit actions for the network popup."""

import json
import os
import subprocess
import sys


class NetworkError(Exception):
    pass


def records(text, columns=None):
    """nmcli terse output escapes both separators and literal backslashes."""
    rows = []
    for line in text.split("\n"):
        if not line:
            continue
        fields, field, escaped = [], [], False
        for char in line:
            if escaped:
                field.append(char)
                escaped = False
            elif char == "\\":
                escaped = True
            elif char == ":":
                fields.append("".join(field))
                field = []
            else:
                field.append(char)
        if escaped:
            raise NetworkError("NetworkManager returned an incomplete record.")
        fields.append("".join(field))
        if columns is not None and len(fields) != columns:
            raise NetworkError("NetworkManager returned an unsupported record.")
        rows.append(fields)
    return rows


def nmcli(args, timeout=15, stdin=None):
    try:
        return subprocess.run(
            ["nmcli", "--colors", "no", "--terse", "--escape", "yes", *args],
            input=stdin,
            stdin=subprocess.DEVNULL if stdin is None else None,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            encoding="utf-8",
            errors="replace",
            env={**os.environ, "LC_ALL": "C.UTF-8"},
            timeout=timeout,
            check=False,
        )
    except FileNotFoundError:
        raise NetworkError("NetworkManager's nmcli command is not installed.") from None
    except OSError:
        raise NetworkError("Could not start NetworkManager's nmcli command. Check permissions.") from None
    except subprocess.TimeoutExpired:
        raise NetworkError("NetworkManager did not respond in time. Try again.") from None


def query(args, columns=None):
    result = nmcli(args)
    if result.returncode:
        raise NetworkError("Could not read NetworkManager state. Check that the service is running.")
    return records(result.stdout, columns)


def wifi_profiles():
    profiles = []
    for name, uuid, kind in query(["-f", "NAME,UUID,TYPE", "connection", "show"], 3):
        if kind != "802-11-wireless":
            continue
        fields = [
            "connection.interface-name",
            "802-11-wireless.ssid",
            "802-11-wireless.hidden",
            "802-11-wireless.bssid",
            "802-11-wireless-security.key-mgmt",
        ]
        settings = dict(query(["-f", ",".join(fields), "connection", "show", "uuid", uuid], 2))
        profiles.append({
            "name": name,
            "uuid": uuid,
            "ssid": settings.get(fields[1], ""),
            "device": settings.get(fields[0], ""),
            "hidden": settings.get(fields[2]) == "yes",
            "bssid": settings.get(fields[3], "").upper(),
            "key": settings.get(fields[4], ""),
        })
    return profiles


def security_kind(security):
    security = security.upper()
    if "802.1X" in security or "EAP" in security:
        return "enterprise"
    if "WEP" in security:
        return "legacy"
    if "WPA" in security:
        return "personal"
    if "OWE" in security:
        return "owe"
    if security in ("", "--"):
        return "open"
    return "unknown"


def profile_kind(profile):
    key = profile["key"]
    if key in ("wpa-psk", "sae"):
        return "personal"
    if key in ("wpa-eap", "wpa-eap-suite-b-192", "ieee8021x"):
        return "enterprise"
    if key == "owe":
        return "owe"
    if key == "none":
        return "legacy"
    return "open" if not key else "unknown"


def build_networks(aps, adapters, profiles, active_by_device):
    grouped = {}
    usable = {adapter["name"] for adapter in adapters if adapter["managed"]}
    for in_use, ssid, bssid, signal, security, device in aps:
        if device not in usable:
            continue
        kind = security_kind(security)
        matching = [
            profile for profile in profiles
            if profile["ssid"] == ssid
            and (not profile["device"] or profile["device"] == device)
            and (not profile["bssid"] or profile["bssid"] == bssid.upper())
            and profile_kind(profile) == kind
        ]
        try:
            signal_strength = max(0, min(100, int(signal or "0")))
        except ValueError:
            raise NetworkError("NetworkManager returned invalid Wi-Fi signal data.") from None
        ap = {
            "name": ssid or "Hidden network",
            "ssid": ssid,
            "bssid": bssid,
            "device": device,
            "signal": signal_strength,
            "security": "Open" if kind == "open" else security,
            "hidden": not ssid,
            "requiresSsid": not ssid,
            "protected": kind == "personal",
            "advanced": kind in ("enterprise", "legacy", "unknown"),
            "available": True,
        }
        for profile in matching or [None]:
            uuid = profile["uuid"] if profile else ""
            entry = {
                **ap,
                "id": device + "/saved/" + uuid if profile else device + "/ap/" + (ssid or bssid) + "/" + kind,
                "active": in_use == "*" and (profile is None or uuid == active_by_device.get(device)),
                "saved": profile is not None,
                "uuid": uuid,
            }
            previous = grouped.get(entry["id"])
            if previous is None or (entry["active"], entry["signal"]) > (previous["active"], previous["signal"]):
                grouped[entry["id"]] = entry
    networks = list(grouped.values())
    represented = {(entry["device"], entry["uuid"]) for entry in networks if entry["saved"]}
    for profile in profiles:
        for device in usable:
            if profile["device"] and profile["device"] != device:
                continue
            if (device, profile["uuid"]) in represented:
                continue
            kind = profile_kind(profile)
            networks.append({
                "id": device + "/saved/" + profile["uuid"],
                "name": profile["ssid"] or profile["name"],
                "ssid": profile["ssid"],
                "bssid": profile["bssid"],
                "device": device,
                "signal": 0,
                "security": {"personal": "WPA", "open": "Open", "owe": "OWE"}.get(kind, "Advanced security"),
                "active": profile["uuid"] == active_by_device.get(device),
                "saved": True,
                "uuid": profile["uuid"],
                "hidden": profile["hidden"],
                "requiresSsid": False,
                "protected": kind == "personal",
                "advanced": kind in ("enterprise", "legacy", "unknown"),
                "available": False,
            })
    return sorted(networks, key=lambda entry: (not entry["active"], not entry["available"], -entry["signal"], entry["name"].casefold(), entry["device"]))


def status():
    general = query(["-f", "STATE,CONNECTIVITY,WIFI,WIFI-HW", "general", "status"], 4)
    if len(general) != 1:
        raise NetworkError("NetworkManager status is unavailable.")
    devices = query(["-f", "DEVICE,TYPE,STATE,CONNECTION,CON-UUID", "device", "status"], 5)
    adapters = [
        {"name": device, "managed": state != "unmanaged", "state": state}
        for device, kind, state, _, _ in devices if kind == "wifi"
    ]
    connections = [
        {"name": name, "type": kind, "device": device, "uuid": uuid}
        for device, kind, state, name, uuid in devices
        if state.startswith("connected") and kind in ("ethernet", "wifi", "gsm", "cdma", "tun", "wireguard")
    ]
    connections.sort(key=lambda connection: {"ethernet": 0, "wifi": 1}.get(connection["type"], 2))
    wifi_enabled = general[0][2] == "enabled"
    profiles = wifi_profiles() if adapters else []
    aps = query(["-f", "IN-USE,SSID,BSSID,SIGNAL,SECURITY,DEVICE", "device", "wifi", "list", "--rescan", "no"], 6) if adapters and wifi_enabled else []
    primary = connections[0] if connections else {}
    return {
        "connected": bool(connections),
        "connectionName": primary.get("name", ""),
        "connectionType": primary.get("type", ""),
        "deviceName": primary.get("device", ""),
        "connections": connections,
        "connectivity": general[0][1],
        "wifiEnabled": wifi_enabled,
        "wifiHardwareEnabled": general[0][3] == "enabled",
        "wifiAvailable": bool(adapters),
        "adapters": adapters,
        "networks": build_networks(aps, adapters, profiles, {item["device"]: item["uuid"] for item in connections}),
    }


def action_error(code):
    return {
        3: "The operation timed out. Check the signal and try again.",
        4: "Connection failed. Check the password, signal, or saved profile.",
        7: "The requested network operation failed. Check permissions or the saved profile.",
        8: "NetworkManager is not running.",
        10: "The network or adapter is no longer available. Refresh and try again.",
    }.get(code, "NetworkManager could not complete the operation. Check permissions or use Advanced settings.")


def request_string(values, field):
    value = values.get(field, "")
    if not isinstance(value, str):
        raise NetworkError(f"Network request field '{field}' must be text.")
    if "\0" in value:
        raise NetworkError(f"Network request field '{field}' cannot contain NUL characters.")
    try:
        value.encode("utf-8")
    except UnicodeEncodeError:
        raise NetworkError(f"Network request field '{field}' contains invalid Unicode.") from None
    return value


def password_file_value(password):
    # NM 1.58 decodes C escapes and strips unescaped boundary whitespace.
    # Three-digit octal UTF-8 bytes preserve both without any literal whitespace.
    return "".join(f"\\{byte:03o}" for byte in password.encode("utf-8"))


def action(request):
    if not isinstance(request, dict):
        raise NetworkError("The network request must be a JSON object.")
    operation = request.get("operation")
    if not isinstance(operation, str):
        raise NetworkError("The network request must specify an operation as text.")
    if operation == "wifi":
        if not isinstance(request.get("enabled"), bool):
            raise NetworkError("The Wi-Fi enabled value must be true or false.")
        result = nmcli(["--wait", "10", "radio", "wifi", "on" if request["enabled"] else "off"])
    elif operation == "scan":
        devices = query(["-f", "DEVICE,TYPE,STATE", "device", "status"], 3)
        failures = []
        for device, kind, state in devices:
            if kind != "wifi" or state in ("unmanaged", "unavailable"):
                continue
            result = nmcli(["--wait", "10", "device", "wifi", "rescan", "ifname", device])
            if result.returncode:
                failures.append(device)
        if failures:
            raise NetworkError("Rescan failed on " + ", ".join(failures) + ". Cached networks are still shown.")
        return {"ok": True}
    elif operation == "connect":
        network = request.get("network")
        if not isinstance(network, dict):
            raise NetworkError("A connection request must include a network object.")
        network = {
            **network,
            **{field: request_string(network, field) for field in ("device", "uuid", "bssid", "ssid")},
        }
        for field in ("advanced", "hidden", "requiresSsid", "protected"):
            if not isinstance(network.get(field, False), bool):
                raise NetworkError(f"Network request field '{field}' must be true or false.")
        password = request_string(request, "password")
        submitted_ssid = request_string(request, "ssid")
        request.pop("password", None)
        if network.get("advanced"):
            raise NetworkError("This network needs Advanced settings for enterprise or legacy security.")
        ssid = submitted_ssid if network.get("requiresSsid") else network["ssid"]
        if any(char in password for char in "\n\r"):
            raise NetworkError("Passwords cannot contain line breaks.")
        if not network.get("device"):
            raise NetworkError("Choose a Wi-Fi adapter first.")
        stdin = None
        if network.get("uuid"):
            args = ["--wait", "45", "connection", "up", "uuid", network["uuid"], "ifname", network["device"]]
            if network.get("bssid"):
                args.extend(["ap", network["bssid"]])
            if password:
                args.extend(["passwd-file", "/dev/stdin"])
                stdin = "802-11-wireless-security.psk:" + password_file_value(password) + "\n"
        else:
            if not ssid:
                raise NetworkError("Enter the hidden network's name.")
            if not network.get("hidden") and not network["bssid"]:
                raise NetworkError("The network access point is missing. Refresh and try again.")
            if network.get("protected") and not password:
                raise NetworkError("Enter the Wi-Fi password.")
            args = ["--wait", "45"]
            if password:
                args.append("--ask")
                stdin = password + "\n"
            args.extend(["device", "wifi", "connect", ssid if network.get("hidden") else network["bssid"], "ifname", network["device"]])
            if network.get("hidden"):
                if network.get("bssid"):
                    args.extend(["bssid", network["bssid"]])
                args.extend(["hidden", "yes"])
        # Secrets only cross anonymous stdin pipes. Never emit nmcli's interactive
        # output or diagnostics: some failure paths may contain user input.
        result = nmcli(args, timeout=55, stdin=stdin)
        password = stdin = ""
        return {
            "ok": result.returncode == 0,
            "error": "" if result.returncode == 0 else action_error(result.returncode),
            "needsPassword": result.returncode in (4, 7) and network.get("protected", False),
        }
    else:
        raise NetworkError("Unknown network operation.")
    return {"ok": result.returncode == 0, "error": "" if result.returncode == 0 else action_error(result.returncode)}


def main():
    try:
        if sys.argv[1:] == ["status"]:
            result = {"ok": True, "state": status()}
        elif sys.argv[1:] == ["action"]:
            result = action(json.loads(sys.stdin.readline(65536)))
        else:
            raise NetworkError("Unknown network helper command.")
    except NetworkError as error:
        result = {"ok": False, "error": str(error)}
    except json.JSONDecodeError:
        result = {"ok": False, "error": "The network request is not valid JSON."}
    except UnicodeError:
        result = {"ok": False, "error": "The network request contains invalid text encoding."}
    except OSError:
        result = {"ok": False, "error": "Could not read the network request or communicate with NetworkManager."}
    print(json.dumps(result, ensure_ascii=False), flush=True)


if __name__ == "__main__":
    main()
