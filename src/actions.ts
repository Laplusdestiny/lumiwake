// 画面からの操作。キーを連打しても 1 つずつ順番に処理する（前の操作の結果を見てから次へ）。
import { api, errorText, type ConflictChoice, type SessionView, type SortAction, type Suggestions } from "./api";
import { notify, store } from "./store";
import { toast } from "./toast";

let chain: Promise<void> = Promise.resolve();
let pending = 0;

export function enqueue(task: () => Promise<void>): Promise<void> {
  pending++;
  chain = chain
    .then(task)
    .catch((e) => toast(errorText(e), "error"))
    .finally(() => {
      pending--;
    });
  return chain;
}

/** まだ処理していない操作が溜まりすぎていたら新しい入力を捨てる（押しっぱなし対策） */
export function busy(): boolean {
  return pending > 2;
}

function isCurrentSuggest(want: { generation: number; index: number }): boolean {
  return store.suggest?.generation === want.generation && store.suggest.index === want.index;
}

/** 画像の AI 候補を取りに行く。表示を止めないよう、結果が来たら差し替える（キャッシュにあれば即座に返る） */
export function loadSuggestions(generation: number, index: number, force: boolean): void {
  const backend = store.config?.config.ai.backend;
  if (!backend || backend === "off") {
    store.suggest = null;
    return;
  }
  const want = { generation, index };
  // 再診断の間は、前の結果を見せたままにする
  const previous = force && isCurrentSuggest(want) ? store.suggest!.view : null;
  store.suggest = { ...want, view: previous, loading: true };
  api
    .getSuggestions(generation, index, force)
    .then((view) => {
      if (!isCurrentSuggest(want)) return;
      store.suggest = { ...want, view, loading: false };
      notify();
    })
    .catch((e) => {
      if (!isCurrentSuggest(want)) return;
      const failed: Suggestions = {
        backend,
        kind: null,
        sendsImages: false,
        cards: [],
        noneOfAbove: null,
        unevaluated: [],
        fromCache: false,
        state: "failed",
        message: errorText(e),
      };
      store.suggest = { ...want, view: failed, loading: false };
      notify();
    });
}

/** 表示中の 1 枚だけ診断し直す（再診断キー） */
export function rediagnose(): void {
  const s = store.session;
  if (!s?.current) return;
  if (store.config?.config.ai.backend === "off") {
    toast("AI 候補は無効です（設定で有効にできます）");
    return;
  }
  loadSuggestions(s.generation, s.current.index, true);
  notify();
}

export function setSession(view: SessionView | null): void {
  store.session = view;
  const cur = view?.current;
  if (view && cur) {
    if (!isCurrentSuggest({ generation: view.generation, index: cur.index })) {
      loadSuggestions(view.generation, cur.index, false);
    }
  } else {
    store.suggest = null;
  }
  if (!view || !cur) {
    store.info = null;
  } else if (store.info?.generation !== view.generation || store.info.index !== cur.index) {
    store.info = { generation: view.generation, index: cur.index, info: null };
    const want = { generation: view.generation, index: cur.index };
    api
      .imageInfo(want.generation, want.index)
      .then((info) => {
        if (store.info?.generation === want.generation && store.info.index === want.index) {
          store.info = { ...want, info };
          notify();
        }
      })
      .catch((e) => {
        if (store.info?.generation === want.generation && store.info.index === want.index) {
          toast(`画像を読み込めません: ${errorText(e)}`, "error");
        }
      });
  }
  notify();
}

export function perform(action: SortAction): Promise<void> {
  return enqueue(async () => {
    const r = await api.perform(action);
    setSession(r.view);
  });
}

export function resolveConflict(choice: ConflictChoice): Promise<void> {
  return enqueue(async () => {
    const r = await api.resolveConflict(choice);
    setSession(r.view);
  });
}

export function cancelConflict(): Promise<void> {
  return enqueue(async () => setSession(await api.cancelConflict()));
}

export function undo(): Promise<void> {
  return enqueue(async () => {
    const r = await api.undo();
    setSession(r.view);
    if (r.message) toast(r.message);
  });
}

export function navigate(forward: boolean): Promise<void> {
  return enqueue(async () => setSession(await api.navigate(forward)));
}

/** 保留した画像などへ移動する */
export function jumpTo(index: number): Promise<void> {
  return enqueue(async () => setSession(await api.jumpTo(index)));
}

export async function startSession(source: string, includeSubdirs: boolean): Promise<void> {
  const view = await api.startSession(source, includeSubdirs);
  store.config = await api.getConfig();
  store.screen = "sort";
  setSession(view);
  if (view.unsupported.length > 0) {
    const exts = [...new Set(view.unsupported.map((p) => p.split(".").pop()?.toUpperCase()))].join(" / ");
    toast(`${exts} の ${view.unsupported.length} 枚は、このビルドでは表示に対応していないため読み込みませんでした`, "info", 6000);
  }
  if (view.total === 0) toast("対象の画像が見つかりませんでした");
}
