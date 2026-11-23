function preferredInState(players, state) {
    const matches = players.filter(player => player.playbackState === state);
    return matches.find(player => player.dbusName.toLowerCase().includes("spotify")) || matches[0] || null;
}

function select(players, playing, paused) {
    return preferredInState(players, playing) || preferredInState(players, paused);
}
