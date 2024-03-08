import {createSignal} from "solid-js";
import "./App.css";
import {Mediainfo} from "./Mediainfo";
import {Player} from "./Player";

const HOSTNAME = new URL(import.meta.env.BASE_URL, location.origin);
// const HOSTNAME = new URL(import.meta.env.BASE_URL, "http://localhost:9005");

export function makeUrl(protocol: "http" | "ws", path: string) {
	let newProtocol: string = protocol;
	if (protocol === "ws") {
		if (location.protocol === "http:") {
			newProtocol = "ws:";
		} else {
			newProtocol = "wss:";
		}
	} else {
		newProtocol = location.protocol;
	}

	let newUrl = new URL(HOSTNAME);
	newUrl.protocol = newProtocol;
	let url = newUrl.toString();
	if (path.startsWith("/")) {
		path = path.slice(1);
	}
	if (!url.endsWith("/")) {
		url += "/";
	}
	// console.log("url + path", newUrl + path);
	return newUrl + path;
}

export default function App() {
	return (
		<div id="container">
			<Mediainfo></Mediainfo>
			<Player></Player>
		</div>
	);
}
