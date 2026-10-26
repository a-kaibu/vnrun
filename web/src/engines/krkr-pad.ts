// KiriKiri's Pad editor, used by KAG when upgrading Config.tjs.
import type { GameFS, SaveErrorDetail } from "../gamefs";
import "./krkr-pad.css";

interface Pad {
  text: string;
  dialog?: HTMLDialogElement;
  editor?: HTMLTextAreaElement;
}

export class Pads {
  private readonly pads = new Map<number, Pad>();

  constructor(
    private readonly fs: GameFS,
    private readonly canvas: HTMLCanvasElement,
  ) {}

  call(id: number, action: string, value: string, title: string, filename: string): string {
    let pad = this.pads.get(id);
    if (!pad) {
      pad = { text: "" };
      this.pads.set(id, pad);
    }
    if (action === "get") return pad.editor?.value ?? pad.text;
    if (action === "visible") return pad.dialog?.open ? "1" : "0";
    if (action === "set") {
      pad.text = value;
      if (pad.editor) pad.editor.value = value;
    } else if (action === "show") {
      this.show(id, pad, title, filename);
    } else if (action === "hide" || action === "close") {
      pad.dialog?.close();
      if (action === "close") {
        pad.dialog?.remove();
        this.pads.delete(id);
      }
    }
    return "";
  }

  private show(id: number, pad: Pad, title: string, filename: string): void {
    if (pad.dialog?.open) return;
    pad.text = pad.editor?.value ?? pad.text;
    pad.dialog?.remove();
    const dialog = document.createElement("dialog");
    dialog.className = "krkr-pad";
    const heading = document.createElement("h2");
    heading.id = `krkr-pad-title-${id}`;
    heading.textContent = title || "テキスト編集";
    dialog.setAttribute("aria-labelledby", heading.id);
    const path = document.createElement("p");
    path.textContent = filename;
    const editor = document.createElement("textarea");
    editor.setAttribute("aria-label", "内容");
    editor.value = pad.text;
    editor.spellcheck = false;
    const status = document.createElement("p");
    status.setAttribute("role", "status");
    const buttons = document.createElement("div");
    const save = document.createElement("button");
    save.type = "button";
    save.textContent = "保存";
    const close = document.createElement("button");
    close.type = "button";
    close.textContent = "閉じる";
    close.addEventListener("click", () => dialog.close());
    const persist = async () => {
      save.disabled = true;
      status.textContent = "保存中…";
      let saveError: unknown;
      let target = "";
      const onError = (event: Event) => {
        const detail = (event as CustomEvent<SaveErrorDetail>).detail;
        if (target && this.fs.resolve(detail.path) === this.fs.resolve(target)) saveError = detail.error;
      };
      window.addEventListener("vnrun-save-error", onError);
      try {
        const local = filename.replace(/^file:\/\/\.?/, "");
        if (!local.startsWith("/game/") || local.split("/").includes(".."))
          throw new Error("ゲームフォルダ内のファイルを指定してください。");
        const text = editor.value;
        // A BOM keeps Japanese text independent of the game's default encoding.
        const bytes = new Uint8Array(2 + text.length * 2);
        const view = new DataView(bytes.buffer);
        view.setUint16(0, 0xfeff, true);
        for (let i = 0; i < text.length; i++) view.setUint16(2 + i * 2, text.charCodeAt(i), true);
        target = local.slice("/game/".length);
        this.fs.write(target, bytes);
        await this.fs.flush();
        if (saveError) throw saveError;
        status.textContent = "保存しました。";
      } catch (error) {
        status.textContent = `保存できませんでした: ${String(error)}`;
      } finally {
        window.removeEventListener("vnrun-save-error", onError);
        save.disabled = false;
      }
    };
    save.addEventListener("click", () => void persist());
    buttons.append(save, close);
    dialog.append(heading, path, editor, status, buttons);
    // SDL also listens on window: text editing must not advance the game underneath.
    dialog.addEventListener("keydown", (event) => event.stopPropagation());
    dialog.addEventListener("keyup", (event) => event.stopPropagation());
    dialog.addEventListener("close", () => this.canvas.focus());
    document.body.append(dialog);
    pad.dialog = dialog;
    pad.editor = editor;
    dialog.showModal();
    editor.focus();
  }
}
