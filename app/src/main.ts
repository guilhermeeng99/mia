import { mount } from "svelte";
// Montserrat — bundled offline (no CDN), the loaded family behind the --font-gilroy token.
import "@fontsource/montserrat/400.css";
import "@fontsource/montserrat/500.css";
import "@fontsource/montserrat/600.css";
import "@fontsource/montserrat/700.css";
import "./styles.css";
import App from "./App.svelte";

const app = mount(App, { target: document.getElementById("app")! });

export default app;
