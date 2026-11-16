import { getCurrentWindow } from "@tauri-apps/api/window";
import { invoke, convertFileSrc } from "@tauri-apps/api/core";
import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { ArrowLeft, ArrowRight, BookOpen, EllipsisVertical, Disc, Minus, Plus } from "lucide-react";

const STORAGE_KEY = "libraryPath";
const MAX_QUANTITY = 9999;
const SERIF = 'Georgia, "Times New Roman", serif';

function Cover({ path }) {
  const [broken, setBroken] = useState(false);
  // A real cover is drawn as-is, so transparent areas show the row behind it.
  if (path && !broken) {
    return (
      <img
        src={convertFileSrc(path)}
        alt=""
        loading="lazy"
        decoding="async"
        className="w-24 h-24 flex-shrink-0 object-contain"
        onError={() => setBroken(true)}
      />
    );
  }
  return (
    <div className="w-24 h-24 flex-shrink-0 rounded-md bg-[#dedcd6] border border-[#d2d0ca] flex items-center justify-center text-[#a09d96]">
      <Disc size={32} strokeWidth={1.5} />
    </div>
  );
}

function ItemRow({ item, onChange }) {
  return (
    <div className={`flex items-center gap-4 px-4 py-2 hover:bg-[#dedcd6] ${item.missing ? "opacity-50" : ""}`}>
      <Cover path={item.cover} />
      <div className="flex-1 min-w-0">
        <div className="text-[16px] text-[#2a2a2a] truncate">{item.name}</div>
        {item.missing && (
          <div className="text-[12px] text-[#7d7b76]">Missing: folder not found</div>
        )}
      </div>
      <div className="flex items-center gap-1 text-[#7d7b76] select-none">
        <button
          className="w-6 h-6 flex items-center justify-center rounded hover:bg-[#d2d0ca] hover:text-[#2a2a2a] disabled:opacity-30 disabled:hover:bg-transparent"
          disabled={item.missing || item.quantity <= 0}
          onClick={() => onChange(item.id, -1)}
          aria-label="Decrease quantity"
        >
          <Minus size={14} />
        </button>
        <span className="w-9 text-center text-[15px] text-[#2a2a2a] tabular-nums">{item.quantity}</span>
        <button
          className="w-6 h-6 flex items-center justify-center rounded hover:bg-[#d2d0ca] hover:text-[#2a2a2a] disabled:opacity-30 disabled:hover:bg-transparent"
          disabled={item.missing || item.quantity >= MAX_QUANTITY}
          onClick={() => onChange(item.id, 1)}
          aria-label="Increase quantity"
        >
          <Plus size={14} />
        </button>
      </div>
    </div>
  );
}

function App() {
  const [libraryPath, setLibraryPath] = useState(null);
  const [items, setItems] = useState([]);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState(null);
  const [warning, setWarning] = useState(null);
  const [searchQuery, setSearchQuery] = useState("");

  useEffect(() => {
    document.documentElement.style.colorScheme = "light";
  }, []);

  const pathRef = useRef(null);
  const queue = useRef(Promise.resolve());

  // Scans and saves run one after another, so they can never overtake each other.
  const enqueue = (fn) => {
    const run = queue.current.then(fn, fn);
    queue.current = run.catch(() => {});
    return run;
  };

  const scan = useCallback((path, silent = false) => {
    if (!silent) setLoading(true);
    return enqueue(() => invoke("scan_library", { path }))
      .then((lib) => {
        if (pathRef.current !== path) return true;
        setItems(lib.items);
        setWarning(lib.warning);
        setError(null);
        return true;
      })
      .catch((e) => {
        setError(String(e));
        return false;
      })
      .finally(() => {
        if (!silent) setLoading(false);
      });
  }, []);

  const loadLibrary = useCallback(
    async (path) => {
      pathRef.current = path;
      setLibraryPath(path);
      setItems([]);
      setWarning(null);
      const ok = await scan(path);
      if (ok) {
        localStorage.setItem(STORAGE_KEY, path);
      } else {
        pathRef.current = null;
        setLibraryPath(null);
        localStorage.removeItem(STORAGE_KEY);
      }
    },
    [scan]
  );

  const openLibrary = useCallback(async () => {
    try {
      const selected = await invoke("open_directory_picker");
      if (selected) await loadLibrary(selected);
    } catch (e) {
      setError(String(e));
    }
  }, [loadLibrary]);

  useEffect(() => {
    getCurrentWindow().show();

    const saved = localStorage.getItem(STORAGE_KEY);
    if (saved) loadLibrary(saved);

    const handleKeyDown = (e) => {
      if (e.ctrlKey && e.shiftKey && e.key === "O") {
        e.preventDefault();
        openLibrary();
      }
    };
    // Coming back to the window (e.g. after adding folders or editing catalog.csv) refreshes the list.
    const handleFocus = () => {
      if (pathRef.current) scan(pathRef.current, true);
    };

    window.addEventListener("keydown", handleKeyDown);
    window.addEventListener("focus", handleFocus);
    return () => {
      window.removeEventListener("keydown", handleKeyDown);
      window.removeEventListener("focus", handleFocus);
    };
  }, [loadLibrary, openLibrary, scan]);

  const changeQuantity = (id, delta) => {
    const current = items.find((i) => i.id === id);
    if (!current) return;
    const next = Math.min(MAX_QUANTITY, Math.max(0, current.quantity + delta));
    if (next === current.quantity) return;
    const path = pathRef.current;

    setItems((prev) => prev.map((i) => (i.id === id ? { ...i, quantity: next } : i)));
    enqueue(() => invoke("set_quantity", { path, id, quantity: next })).catch((e) => {
      // Show what is really saved, then keep the error visible.
      scan(path, true).then(() => setError(String(e)));
    });
  };

  // Items arrive sorted by format, then name; group them for display.
  const groups = useMemo(() => {
    const q = searchQuery.trim().toLowerCase();
    const shown = q ? items.filter((i) => i.name.toLowerCase().includes(q)) : items;
    const out = [];
    for (const item of shown) {
      const last = out[out.length - 1];
      if (last && last.format === item.format) last.items.push(item);
      else out.push({ format: item.format, items: [item] });
    }
    return out;
  }, [items, searchQuery]);

  return (
    <div className="relative flex flex-col h-screen">
      <header className="bg-[#e9e7e2] text-[#2a2a2a] h-[40px] flex items-center border-[#d2d0ca] border-b-[0.4px] px-2">
        <div className="flex items-center h-full ml-1 text-[#a09d96]">
          <button className="w-[25px] h-full flex justify-center items-center cursor-default" disabled>
            <ArrowLeft size={15} strokeWidth={1.75} />
          </button>
          <button className="w-[25px] h-full flex justify-center items-center cursor-default" disabled>
            <ArrowRight size={15} strokeWidth={1.75} />
          </button>
        </div>

        <input
          value={searchQuery}
          onChange={(e) => setSearchQuery(e.target.value)}
          placeholder="Search"
          spellCheck={false}
          className="flex-1 min-w-0 mx-2 h-[30px] bg-transparent outline-none border border-[#d2d0ca] focus:border-[#a09d96] rounded-md px-3 text-[13px] text-[#2a2a2a] placeholder-[#85827c]"
        />

        <div className="flex items-center h-full mr-1 text-[#85827c]">
          <button className="w-[25px] h-full flex justify-center items-center hover:text-[#2a2a2a]">
            <BookOpen size={15} strokeWidth={1.75} />
          </button>
          <button className="w-[25px] h-full flex justify-center items-center hover:text-[#2a2a2a]">
            <EllipsisVertical size={15} strokeWidth={1.75} />
          </button>
        </div>
      </header>

      <div className="main flex-1 flex flex-col min-h-0 bg-[#e9e7e2] text-[#625f5a]" style={{ fontFamily: SERIF }}>
        {(error || warning) && (
          <div className="px-4 py-2 text-[12px] border-b border-[#d2d0ca] whitespace-pre-line">
            {error && <div className="text-[#b3362b]">{error}</div>}
            {warning && <div className="text-[#8a6a00]">{warning}</div>}
          </div>
        )}

        {!libraryPath ? (
          <div
            className="flex-1 flex items-center justify-center flex-col gap-3 text-[13px] leading-5 text-[#7d7b76] select-none"
          >
            <div
              className="cursor-pointer hover:text-[#2a2a2a] transition-colors"
              onClick={openLibrary}
            >
              Open existing vault (Ctrl + Shift + O)
            </div>
            <div
              className="cursor-pointer hover:text-[#2a2a2a] transition-colors"
              onClick={() => getCurrentWindow().close()}
            >
              Close
            </div>
          </div>
        ) : loading && items.length === 0 ? (
          <div className="flex-1 flex items-center justify-center text-[13px]">Scanning…</div>
        ) : items.length === 0 ? (
          <div className="flex-1 flex items-center justify-center text-[13px] text-center px-8">
            No items found. Each item is a folder inside a format folder, e.g. CD&amp;DVD/Item name/cover.png
          </div>
        ) : groups.length === 0 ? (
          <div className="flex-1 flex items-center justify-center text-[13px]">No matches</div>
        ) : (
          <div className="flex-1 min-h-0 overflow-y-auto pb-4">
            {groups.map((g) => (
              <section key={g.format}>
                <div className="sticky top-0 z-10 bg-[#e9e7e2] px-4 pt-4 pb-1 text-[12px] uppercase tracking-wide text-[#7d7b76]">
                  {g.format} · {g.items.length}
                </div>
                {g.items.map((item) => (
                  <ItemRow key={item.id} item={item} onChange={changeQuantity} />
                ))}
              </section>
            ))}
          </div>
        )}
      </div>
    </div>
  );
}

export default App;