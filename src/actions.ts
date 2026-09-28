// 画面からの操作。キーを連打しても 1 つずつ順番に処理する（前の操作の結果を見てから次へ）。
import { api, errorText, type ConflictChoice, type SessionView, type SortAction } from "./api";
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

export function setSession(view: SessionView | null): void {
  store.session = view;
  const cur = view?.current;
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
