// Boots the egui spike app on the full-viewport canvas.
import init, { WebHandle } from "./spike3.js";

await init("./spike3_bg.wasm");
const handle = new WebHandle();
const canvas = document.getElementById("the_canvas_id");
try {
  await handle.start(canvas);
  console.log("spike3: app started");
} catch (e) {
  console.error("spike3: start failed", e);
}
