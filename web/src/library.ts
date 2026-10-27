import "./library.css";

interface Game {
  title: string;
  engine?: string;
  error?: string;
  url: string;
}

const status = document.querySelector<HTMLElement>("#status")!;
const games = document.querySelector<HTMLUListElement>("#games")!;
const refresh = document.querySelector<HTMLButtonElement>("#refresh")!;

async function loadGames(): Promise<void> {
  refresh.disabled = true;
  status.textContent = "作品を読み込んでいます…";
  try {
    const response = await fetch("/api/games", { cache: "no-store" });
    if (!response.ok) throw new Error(`HTTP ${response.status}`);
    const entries = (await response.json()) as Game[];
    games.replaceChildren();
    for (const game of entries) {
      const item = document.createElement("li");
      const title = document.createElement(game.error ? "span" : "a");
      title.textContent = game.title;
      if (title instanceof HTMLAnchorElement) {
        title.href = `./index.html?game=${encodeURIComponent(game.url)}&back=${encodeURIComponent("/")}`;
      }
      const detail = document.createElement("p");
      detail.textContent = game.error ? `準備できません: ${game.error}` : game.engine || "";
      if (game.error) detail.className = "error";
      item.append(title, detail);
      games.append(item);
    }
    status.textContent = entries.length
      ? `${entries.length} 作品`
      : "作品がありません。ライブラリにゲームを追加してください。";
  } catch (error) {
    status.textContent = `一覧を取得できません: ${String(error)}`;
  } finally {
    refresh.disabled = false;
  }
}

refresh.addEventListener("click", () => void loadGames());
void loadGames();
