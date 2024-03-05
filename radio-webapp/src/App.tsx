import {createSignal} from "solid-js";
import "./App.css";
import {Mediainfo} from "./Mediainfo";
import {Player} from "./Player";

// export const HOSTNAME = "localhost:9005";
export const HOSTNAME = location.host;

export default function App() {
	return (
		<div id="container">
			<Mediainfo></Mediainfo>
			<Player></Player>
		</div>
	);
}
