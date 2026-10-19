// ANNOTA-20: the instrumented installer. Same as `pnpm build:installer`, plus the page's timing marks
// (VITE_TIMINGS=1) and the `timings` cargo feature (QPC marks and the tray's dump items).
import { spawnSync } from "node:child_process";

const run = (command) => {
  const { status } = spawnSync(command, { stdio: "inherit", shell: true, env: { ...process.env, VITE_TIMINGS: "1" } });
  if (status !== 0) process.exit(status ?? 1);
};

run("pnpm exec vite build");
run("pnpm exec tauri build --bundles nsis --features timings");
