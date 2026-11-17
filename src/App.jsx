import { getCurrentWindow } from "@tauri-apps/api/window";
import { invoke, convertFileSrc } from "@tauri-apps/api/core";
import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { ArrowLeft, ArrowRight, BookOpen, Pencil, EllipsisVertical, Disc, Minus, Plus } from "lucide-react";

const STORAGE_KEY = "libraryPath";
const MAX_QUANTITY = 9999;
// How covers are shown: "off", "side" (small image left of the title) or "background" (disc peeking up from the bottom of each item)
const COVER_STYLE = "background";
const CONTENT_WIDTH = "mx-auto w-full max-w-[800px]";
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

// The disc sits behind the item, mostly cut off by the bottom edge, and a bit see-through.
function CoverBackdrop({ path }) {
  const [broken, setBroken] = useState(false);
  if (!path || broken) return null;
  return (
    <img
      src={convertFileSrc(path)}
      alt=""
      draggable={false}
      className="absolute right-10 w-[200px] h-[200px] object-contain opacity-30 pointer-events-none select-none"
      style={{ top: "calc(100% - 52px)" }}
      onError={() => setBroken(true)}
    />
  );
}

const keyOf = (item) => `${item.format}/${item.folder}`;

// Plain text while reading; a text box while editing (Enter or click away saves, Escape cancels).
function EditableText({ value, editing, onCommit, allowEmpty = false, placeholder, maxLength, textClass, inputClass }) {
  const [draft, setDraft] = useState(value);
  const cancelled = useRef(false);
  useEffect(() => setDraft(value), [value]);

  if (!editing) {
    return <span className={textClass}>{value}</span>;
  }

  const commit = () => {
    if (cancelled.current) {
      cancelled.current = false;
      setDraft(value);
      return;
    }
    const next = draft.trim();
    if (!next && !allowEmpty) setDraft(value);
    else if (next !== value) onCommit(next);
    else setDraft(value);
  };

  return (
    <input
      value={draft}
      onChange={(e) => setDraft(e.target.value)}
      onBlur={commit}
      onKeyDown={(e) => {
        if (e.key === "Enter") e.currentTarget.blur();
        if (e.key === "Escape") {
          cancelled.current = true;
          e.currentTarget.blur();
        }
      }}
      placeholder={placeholder}
      maxLength={maxLength}
      spellCheck={false}
      className={`bg-transparent outline-none border border-[#d2d0ca] focus:border-[#a09d96] rounded-md px-2 ${inputClass}`}
    />
  );
}

function ItemRow({ item, editing, onQuantity, onRename, onDate }) {
  const stepper =
    "w-5 h-5 flex items-center justify-center rounded hover:bg-[#d2d0ca] hover:text-[#2a2a2a] disabled:opacity-30 disabled:hover:bg-transparent";
  return (
    <div
      className={`relative overflow-hidden flex items-center gap-4 px-4 hover:bg-[#dedcd6] border-b-[0.4px] border-[#d2d0ca] last:border-b-0 ${
        COVER_STYLE === "side" ? "py-2" : "py-3"
      }`}
    >
      {COVER_STYLE === "side" && <Cover path={item.cover} />}
      {COVER_STYLE === "background" && <CoverBackdrop path={item.cover} />}
      <div className="relative flex-1 min-w-0">
        {/* Title */}
        {editing ? (
          <EditableText
            value={item.name}
            editing
            onCommit={(name) => onRename(item, name)}
            maxLength={200}
            inputClass="w-full py-1 text-[14px] text-[#2a2a2a]"
          />
        ) : (
          <div className="text-[14px] text-[#2a2a2a] truncate">{item.name}</div>
        )}

        {/* Facts under the title: Date ...  Qt ... */}
        <div className={`flex items-center gap-4 text-[13px] text-[#7d7b76] ${editing ? "mt-2" : "mt-0.5"}`}>
          {(editing || item.date) && (
            <div className="flex items-center gap-1.5">
              <span>Date</span>
              <EditableText
                value={item.date}
                editing={editing}
                allowEmpty
                onCommit={(date) => onDate(item, date)}
                placeholder="DD.MM.YYYY"
                maxLength={40}
                textClass="text-[#2a2a2a]"
                inputClass="w-32 py-0.5 text-[13px] text-[#2a2a2a]"
              />
            </div>
          )}
          <div className="flex items-center gap-1.5">
            <span>Qt</span>
            {editing && (
              <button
                className={stepper}
                disabled={item.quantity <= 0}
                onClick={() => onQuantity(item, -1)}
                aria-label="Decrease quantity"
              >
                <Minus size={12} />
              </button>
            )}
            <span className="text-[#2a2a2a] tabular-nums">{item.quantity}</span>
            {editing && (
              <button
                className={stepper}
                disabled={item.quantity >= MAX_QUANTITY}
                onClick={() => onQuantity(item, 1)}
                aria-label="Increase quantity"
              >
                <Plus size={12} />
              </button>
            )}
          </div>
        </div>
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
  const [editing, setEditing] = useState(false); // reading mode by default

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
    // Coming back to the window (e.g. after adding folders or editing an info.json) refreshes the list.
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

  // Optimistic update, then save; if saving fails, reload what is really on disk and show the error.
  const save = (item, patch, command, args) => {
    const path = pathRef.current;
    const { format, folder } = item;
    setItems((prev) =>
      prev.map((i) => (i.format === format && i.folder === folder ? { ...i, ...patch } : i))
    );
    enqueue(() => invoke(command, { path, format, folder, ...args })).catch((e) => {
      scan(path, true).then(() => setError(String(e)));
    });
  };

  const changeQuantity = (item, delta) => {
    const next = Math.min(MAX_QUANTITY, Math.max(0, item.quantity + delta));
    if (next !== item.quantity) save(item, { quantity: next }, "set_quantity", { quantity: next });
  };

  const renameItem = (item, name) => {
    save(item, { name }, "set_name", { name });
  };

  const changeDate = (item, date) => {
    save(item, { date }, "set_date", { date });
  };

  // One list, ordered by format and then name (so a renamed item moves to its place).
  const shown = useMemo(() => {
    const q = searchQuery.trim().toLowerCase();
    const list = q ? items.filter((i) => i.name.toLowerCase().includes(q)) : items;
    const opts = { numeric: true, sensitivity: "base" };
    return [...list].sort(
      (a, b) => a.format.localeCompare(b.format, undefined, opts) || a.name.localeCompare(b.name, undefined, opts)
    );
  }, [items, searchQuery]);

  return (
    <div className="relative flex flex-col h-screen">
      <header className="bg-[#e9e7e2] text-[#2a2a2a] h-[40px]">
        <div className={`${CONTENT_WIDTH} h-full flex items-center px-2 border-b-[0.4px] border-[#d2d0ca]`}>
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
            <button
              className={`w-[25px] h-full flex justify-center items-center hover:text-[#2a2a2a] ${
                editing ? "text-[#2a2a2a]" : ""
              }`}
              onClick={() => setEditing((e) => !e)}
              title={editing ? "Editing: click for reading mode" : "Reading: click to edit"}
              aria-label={editing ? "Switch to reading mode" : "Switch to editing mode"}
            >
              {editing ? <Pencil size={15} strokeWidth={1.75} /> : <BookOpen size={15} strokeWidth={1.75} />}
            </button>
            <button className="w-[25px] h-full flex justify-center items-center hover:text-[#2a2a2a]">
              <EllipsisVertical size={15} strokeWidth={1.75} />
            </button>
          </div>
        </div>
      </header>

      <div className="main flex-1 flex flex-col min-h-0 bg-[#e9e7e2] text-[#625f5a]" style={{ fontFamily: SERIF }}>
        {(error || warning) && (
          <div>
            <div className={`${CONTENT_WIDTH} px-4 py-2 text-[12px] whitespace-pre-line border-b border-[#d2d0ca]`}>
              {error && <div className="text-[#b3362b]">{error}</div>}
              {warning && <div className="text-[#8a6a00]">{warning}</div>}
            </div>
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
            No items found. Each item is a folder inside a format folder, e.g. CD&amp;DVD/48213907
          </div>
        ) : shown.length === 0 ? (
          <div className="flex-1 flex items-center justify-center text-[13px]">No matches</div>
        ) : (
          <div className="flex-1 min-h-0 overflow-y-auto py-2">
            <div className={CONTENT_WIDTH}>
            {shown.map((item) => (
              <ItemRow
                key={keyOf(item)}
                item={item}
                editing={editing}
                onQuantity={changeQuantity}
                onRename={renameItem}
                onDate={changeDate}
              />
            ))}
            </div>
          </div>
        )}
      </div>
    </div>
  );
}

export default App;