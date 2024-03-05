import {JSX, Show, createSignal} from "solid-js";
import {HOSTNAME} from "./App";

async function fetchMediainfo() {
	return await fetch(`http://${HOSTNAME}/mediainfo`)
		.then((v) => v.json())
		.catch(() => {});
}

function snakeCaseToTitleCase(str: string) {
	return str
		.split("_")
		.map((v) => v.charAt(0).toUpperCase() + v.slice(1))
		.join(" ");
}

// rotate between foo and bar to actually update the image when url changes
function makeImageUrl(other: boolean) {
	// return `http://${HOSTNAME}/album_art?` + (other ? "bar" : "foo");
	return `http://${HOSTNAME}/album_art?t=${Math.random()}`;
}

export function Mediainfo() {
	const [mediainfo, setMediainfo] = createSignal<null | Array<Map<string, any>>>(null);
	const [useOtherUrl, setImageUrl] = createSignal(false);

	// const ws = new WebSocket("ws://" + window.location.host + "/mediainfo/ws");
	const ws = new WebSocket(`ws://${HOSTNAME}/mediainfo/ws`);
	ws.onmessage = async () => {
		setMediainfo(await fetchMediainfo());
		setImageUrl((v) => !v);
	};

	ws.onmessage(null as any);

	return (
		<div id="mediainfo">
			<div id="image">
				<img src={makeImageUrl(useOtherUrl())} alt="Album art" />
			</div>
			<div id="info">
				<ul>
					<Show when={mediainfo()}>
						{Object.entries(mediainfo()![0]).map(([key, value]) => {
							return <MediainfoEntry key={key} value={value} />;
						})}
					</Show>
				</ul>
			</div>
		</div>
	);
}

function MediainfoEntry(props: {key: string; value: any}) {
	return (
		<li>
			<span>{snakeCaseToTitleCase(props.key)}: </span>
			<span>{props.value}</span>
		</li>
	);
}
