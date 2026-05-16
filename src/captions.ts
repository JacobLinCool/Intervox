import { mount } from "svelte";
import CaptionsWindow from "./CaptionsWindow.svelte";

const app = mount(CaptionsWindow, { target: document.getElementById("captions-app")! });

export default app;
