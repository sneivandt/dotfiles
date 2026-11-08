#!/usr/bin/env python3
"""Exercise Quickshell output and lock lifecycles in a disposable Wayland session."""

import argparse
import json
import select
import os
from pathlib import Path
import subprocess
import tempfile
import time


SOURCE = Path(__file__).resolve().parents[4]


def main():
    if not Path('/.dockerenv').exists() or os.geteuid() == 0:
        raise RuntimeError('run as the fixture user inside the desktop CI container')
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--native-hyprland', action='store_true',
                        help='require nested Hyprland and the tracked Hyprlock config; needs GPU-backed buffer allocation')
    native = parser.parse_args().native_hyprland
    with tempfile.TemporaryDirectory(prefix='q') as temporary:
        root = Path(temporary)
        runtime = root / 'r'
        runtime.mkdir(mode=0o700)
        env = dict(os.environ)
        for key in ('DISPLAY', 'WAYLAND_DISPLAY', 'HYPRLAND_INSTANCE_SIGNATURE'):
            env.pop(key, None)
        env.update(XDG_RUNTIME_DIR=str(runtime), XDG_CONFIG_HOME=str(root / 'config'),
                   XDG_CACHE_HOME=str(root / 'cache'), WLR_BACKENDS='headless',
                   WLR_RENDERER='gles2' if native else 'pixman', WLR_LIBINPUT_NO_DEVICES='1',
                   LIBGL_ALWAYS_SOFTWARE='0' if native else '1', QT_QUICK_BACKEND='software',
                   HYPRLAND_TRACE='1', HYPRLAND_LOG_WLR='1', HYPRLAND_NO_SD_NOTIFY='1', HYPRLAND_NO_SD_VARS='1')
        processes = []
        logs = []

        def start(name, args):
            path = root / (name + '.log')
            log = path.open('w')
            logs.append((path, log))
            process = subprocess.Popen(args, env=env, stdin=subprocess.DEVNULL,
                                       stdout=log, stderr=subprocess.STDOUT)
            processes.append(process)
            return process

        def wait_for(description, predicate):
            deadline = time.monotonic() + 25
            while time.monotonic() < deadline:
                for process in processes:
                    if process.poll() is not None:
                        raise RuntimeError(f'child {process.args} exited {process.returncode}')
                if predicate():
                    return
                time.sleep(0.2)
            raise RuntimeError('timed out: ' + description)

        def ctl(*args):
            return subprocess.check_output(['hyprctl', *args], env=env, text=True, timeout=5).strip()

        def query(name):
            return json.loads(ctl('-j', name))

        def bar_on(name):
            return 'dotfiles-bar' in json.dumps(query('layers').get(name, {}))

        try:
            sway_config = root / 'sway.conf'
            sway_config.write_text('output * mode 1280x720\n')
            start('sway', ['sway', '-c', str(sway_config)])
            wait_for('parent compositor', lambda: (runtime / 'wayland-1').is_socket())
            env['WAYLAND_DISPLAY'] = 'wayland-1'
            if native:
                hypr_config = root / 'hyprland.lua'
                hypr_config.write_text('hl.monitor({ output = "", mode = "preferred", position = "auto", scale = 1 })\nhl.config({ debug = { enable_stdout_logs = true, disable_logs = false } })\n')
                start('hyprland', ['Hyprland', '--config', str(hypr_config)])
                wait_for('nested compositor IPC', lambda: bool(list(runtime.glob('hypr/*/.socket.sock'))))
                socket = next(runtime.glob('hypr/*/.socket.sock'))
                env['HYPRLAND_INSTANCE_SIGNATURE'] = socket.parent.name
                wait_for('nested Wayland socket', lambda: any(path.is_socket() and path.name != 'wayland-1'
                                                            for path in runtime.glob('wayland-*')))
                env['WAYLAND_DISPLAY'] = next(path.name for path in runtime.glob('wayland-*')
                                               if path.is_socket() and path.name != 'wayland-1')
                assert not ctl('configerrors'), 'fixture compositor configuration must be valid'
                original = query('monitors')[0]['name']
                start('quickshell', ['quickshell', '--no-duplicate', '--path',
                                    str(SOURCE / 'symlinks/config/quickshell')])
                wait_for('real bar surface', lambda: bar_on(original))
                # Use the same native dispatcher expression as Bar.qml.
                ctl('dispatch', 'hl.dsp.focus({ workspace = 2 })')
                wait_for('workspace activation', lambda: query('activeworkspace')['id'] == 2)
                ctl('output', 'create', 'headless', 'TEST-OUTPUT')
                wait_for('bar on added output', lambda: bar_on('TEST-OUTPUT'))
                ctl('eval', 'hl.monitor({ output = "TEST-OUTPUT", mode = "1024x768@60", position = "auto", scale = 1 })')
                wait_for('output resize', lambda: any(m['name'] == 'TEST-OUTPUT' and m['width'] == 1024
                                                       for m in query('monitors')))
                ctl('output', 'remove', 'TEST-OUTPUT')
                wait_for('output removal', lambda: all(m['name'] != 'TEST-OUTPUT' for m in query('monitors')))
                wait_for('surviving bar after hotplug', lambda: bar_on(original))
                # The fixture has no systemd user manager. Wrapper/service behavior
                # is covered separately; here the real locker must acquire a lock.
                start('hyprlock', ['hyprlock', '--no-fade-in', '--config',
                                  str(SOURCE / 'symlinks/config/hypr/hyprlock.conf')])
                wait_for('native session lock', lambda: query('status')['locked'])
                print('Native desktop acceptance passed: bar, workspace, output add/resize/remove, session lock')
            else:
                env['SWAYSOCK'] = str(next(runtime.glob('sway-ipc.*.sock')))

                def sway(*args):
                    return json.loads(subprocess.check_output(['swaymsg', '-r', *args], env=env,
                                                             text=True, timeout=5))

                def active_outputs():
                    return [output for output in sway('-t', 'get_outputs') if output['active']]

                def reserved_bar(name):
                    output = next((output for output in active_outputs() if output['name'] == name), None)
                    return output is not None and any(workspace['output'] == name and
                        workspace['rect']['y'] > output['rect']['y']
                        for workspace in sway('-t', 'get_workspaces'))

                original = active_outputs()[0]['name']
                start('quickshell', ['quickshell', '--no-duplicate', '--path',
                                    str(SOURCE / 'symlinks/config/quickshell')])
                wait_for('real bar reservation', lambda: reserved_bar(original))
                assert all(result['success'] for result in sway('workspace', '2'))
                wait_for('workspace activation', lambda: any(workspace['name'] == '2' and workspace['focused']
                                                            for workspace in sway('-t', 'get_workspaces')))
                assert all(result['success'] for result in sway('create_output'))
                wait_for('second output', lambda: len(active_outputs()) == 2)
                added = next(output['name'] for output in active_outputs() if output['name'] != original)
                wait_for('bar reservation on new output', lambda: reserved_bar(added))
                assert all(result['success'] for result in sway('output', added, 'mode', '1024x768'))
                wait_for('resized output', lambda: any(output['name'] == added and output['rect']['width'] == 1024
                                                      for output in active_outputs()))
                assert all(result['success'] for result in sway('output', added, 'disable'))
                wait_for('disabled output removed from active model', lambda: len(active_outputs()) == 1)
                wait_for('surviving bar after output changes', lambda: reserved_bar(original))
                ready, writer = os.pipe()
                try:
                    log_path = root / 'swaylock.log'
                    log = log_path.open('w')
                    logs.append((log_path, log))
                    locker = subprocess.Popen(['swaylock', '--ready-fd', str(writer), '--color', '000000'],
                        env=env, stdin=subprocess.DEVNULL, stdout=log, stderr=subprocess.STDOUT,
                        pass_fds=(writer,))
                    processes.append(locker)
                    os.close(writer)
                    writer = None
                    assert select.select([ready], [], [], 20)[0], 'locker must acknowledge acquisition'
                    assert os.read(ready, 1) == b'\n', 'locker must acquire the native session lock'
                    assert locker.poll() is None, 'locker must stay alive after acquisition'
                finally:
                    os.close(ready)
                    if writer is not None:
                        os.close(writer)
                print('Software Wayland acceptance passed: bar reservations, workspace, output add/resize/disable, session lock')
        finally:
            for process in reversed(processes):
                if process.poll() is None:
                    process.terminate()
            for process in reversed(processes):
                try:
                    process.wait(timeout=5)
                except subprocess.TimeoutExpired:
                    process.kill()
                    process.wait(timeout=5)
            for path, log in logs:
                log.close()
                print(f'--- {path.name} ---\n{path.read_text(errors="replace")}')
            for path in root.rglob('hyprland.log'):
                print(path.read_text(errors='replace'))


if __name__ == '__main__':
    main()
