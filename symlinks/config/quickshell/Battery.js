function icon(available, charging, fullyCharged, charge) {
    if (charging)
        return "\uf0e7";
    if (!available)
        return "\uf128";
    if (fullyCharged)
        return "\uf240";
    if (charge <= 0.15)
        return "\uf244";
    if (charge <= 0.35)
        return "\uf243";
    if (charge <= 0.6)
        return "\uf242";
    if (charge <= 0.85)
        return "\uf241";
    return "\uf240";
}
