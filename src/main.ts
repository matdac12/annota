import { mountEditorPage, startEditor } from "./editor-page";
import { tauriIpc } from "./ipc";

startEditor(window, mountEditorPage(document), tauriIpc);
