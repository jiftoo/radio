import {JSX, Show, createSignal} from "solid-js";
import {makeUrl} from "./App";
import youtubeIcon from "./assets/youtube.png";
import touhoudbIcon from "./assets/touhoudb.jpg";
import noImage from "./assets/noimage.png";

async function fetchMediainfo() {
	return await fetch(makeUrl("http", "/mediainfo"))
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
function makeImageUrl(filename: string) {
	return makeUrl("http", `/album_art?n=${encodeURIComponent(filename)}`);
}

export function Mediainfo() {
	const [mediainfo, setMediainfo] = createSignal<null | Array<Record<string, string>>>(null);

	// const ws = new WebSocket("wss://" + window.location.host + "/mediainfo/ws");
	const ws = new WebSocket(makeUrl("ws", "/mediainfo/ws"));
	ws.onmessage = async () => {
		setMediainfo(await fetchMediainfo());
		document.title = "Now playing: " + joinTitleDate(bestTitleData(mediainfo()![0]));
	};

	ws.onmessage(null as any);

	const lastSong = () => {
		return mediainfo()![0];
	};

	return (
		<div id="mediainfo">
			<div id="image">
				<Show when={mediainfo()}>
					<img
						src={makeImageUrl(lastSong().filename)}
						onError={(ev) => {
							ev.currentTarget.src = noImage;
						}}
					/>
				</Show>
			</div>
			<div id="info">
				<ul>
					<Show when={mediainfo()}>
						{Object.entries(lastSong()).map(([key, value]) => {
							return <MediainfoEntry key={key} value={value} />;
						})}
						<MediaLinks mediainfo={lastSong()} />
					</Show>
				</ul>
			</div>
		</div>
	);
}

function MediainfoEntry(props: {key: string; value: string}) {
	const value = () => {
		if (!props.value) {
			return "N/A";
		} else if (props.key === "bitrate") {
			return Math.floor(+props.value / 1000) + "kbps";
		}
		return props.value;
	};
	return (
		<li>
			<span>{snakeCaseToTitleCase(props.key)}: </span>
			<span>{value()}</span>
		</li>
	);
}

function bestTitleData(mediainfo: Record<string, string>): {title: string; artist: string | null} {
	let title = null;
	if (mediainfo.title) {
		title = mediainfo.title;
	} else if (mediainfo.publisher) {
		title = mediainfo.publisher;
	} else {
		title = mediainfo.filename;
	}

	let artist = null;
	if (mediainfo.album_artist) {
		artist = mediainfo.album_artist;
	} else if (mediainfo.artist) {
		artist = mediainfo.artist;
	}

	return {title, artist};
}

function joinTitleDate(data: {title: string; artist: string | null}) {
	return (data.artist ? data.artist + " - " : "") + data.title;
}

function MediaLinks(props: {mediainfo: Record<string, string>}) {
	const youtube = () => {
		const titleData = bestTitleData(props.mediainfo);
		const query = joinTitleDate(titleData);
		const url = `https://www.youtube.com/results?search_query=${encodeURIComponent(query)}`;
		return {url, query};
	};
	const touhoudb = () => {
		const {title} = bestTitleData(props.mediainfo);
		const url = `https://touhoudb.com/Search?searchType=Song&filter=${encodeURIComponent(title)}`;
		return {url, query: title};
	};
	return (
		<div id="media-links">
			<a title={`Search "${youtube().query}" on YouTube`} target="_blank" href={youtube().url}>
				<img src={youtubeIcon}></img>
			</a>
			<a title={`Search "${touhoudb().query}" on TouhouDB`} target="_blank" href={touhoudb().url}>
				<img src={touhoudbIcon}></img>
			</a>
		</div>
	);
}
