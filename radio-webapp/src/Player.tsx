import {JSX, createSignal} from "solid-js";
import {makeUrl} from "./App";

export function Player() {
	const [playing, setPlaying] = createSignal(false);
	const [volume, setVolume] = createSignal(33);

	let audioRef: undefined | HTMLAudioElement;

	const changeVolume: JSX.EventHandlerUnion<HTMLInputElement, InputEvent> = (ev) => {
		setVolume(+ev.currentTarget.value / 100);
		audioRef!.volume = +ev.currentTarget.value / 100;
	};

	const togglePlay = () => {
		const audio = audioRef!;
		setPlaying((v) => {
			if (v) {
				audio.pause();
			} else {
				if (isFinite(audio.duration)) {
					audio.currentTime = Math.max(audio.duration - 1, 0);
				}
				audio.play();
			}
			return !v;
		});
	};

	return (
		<div id="player">
			<div id="controls">
				<button id="play" onClick={togglePlay}>
					{playing() ? "Pause" : "Play"}
				</button>
				<span>Volume:</span>
				<input id="volume" type="range" min="0" max="100" step="1" value="100" onInput={changeVolume}></input>
				<audio ref={audioRef} src={makeUrl("http", "/stream")}></audio>
			</div>
		</div>
	);
}
