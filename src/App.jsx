import { getCurrentWindow } from "@tauri-apps/api/window";
import { invoke } from "@tauri-apps/api/core";
import { useEffect, useState } from "react";
import { Plus, X } from "lucide-react";

function App() {
  const [vaultPath, setVaultPath] = useState(null);
  const [files, setFiles] = useState([]);
  const [openTabs, setOpenTabs] = useState([]);
  const [activeTab, setActiveTab] = useState(null);
  const [noteContent, setNoteContent] = useState("");
  const [showNoteList, setShowNoteList] = useState(false);
  const [expandedFolders, setExpandedFolders] = useState(new Set());

  useEffect(() => {
    getCurrentWindow().show();
    
    // Listen for keyboard shortcuts
    const handleKeyDown = (e) => {
      if (e.ctrlKey && e.shiftKey && e.key === 'O') {
        e.preventDefault();
        openVault();
      }
    };
    
    window.addEventListener('keydown', handleKeyDown);
    return () => window.removeEventListener('keydown', handleKeyDown);
  }, []);

  // Helper function to remove .md extension from display name
  const removeMarkdownExtension = (fileName) => {
    return fileName.endsWith('.md') ? fileName.slice(0, -3) : fileName;
  };

  const openVault = async () => {
    try {
      const selected = await invoke('open_directory_picker');
      if (selected) {
        setVaultPath(selected);
        loadFiles(selected);
      }
    } catch (error) {
      console.error('Failed to open vault:', error);
    }
  };

  const loadFiles = async (path) => {
    try {
      const fileList = await invoke('get_vault_files', { path });
      setFiles(fileList);
    } catch (error) {
      console.error('Failed to load files:', error);
    }
  };

  const openNote = async (filePath, fileName) => {
    try {
      // Check if tab is already open
      if (openTabs.find(tab => tab.path === filePath)) {
        setActiveTab(filePath);
        setShowNoteList(false);
        return;
      }

      const content = await invoke('read_note', { path: filePath });
      const displayName = removeMarkdownExtension(fileName);
      const newTab = { path: filePath, name: displayName, content };
      
      setOpenTabs(prev => [...prev, newTab]);
      setActiveTab(filePath);
      setNoteContent(content);
      setShowNoteList(false);
    } catch (error) {
      console.error('Failed to open note:', error);
    }
  };

  const closeTab = (tabPath) => {
    setOpenTabs(prev => prev.filter(tab => tab.path !== tabPath));
    if (activeTab === tabPath) {
      const remainingTabs = openTabs.filter(tab => tab.path !== tabPath);
      if (remainingTabs.length > 0) {
        setActiveTab(remainingTabs[0].path);
        setNoteContent(remainingTabs[0].content);
      } else {
        setActiveTab(null);
        setNoteContent("");
      }
    }
  };

  const switchTab = (tabPath) => {
    setActiveTab(tabPath);
    const tab = openTabs.find(t => t.path === tabPath);
    if (tab) {
      setNoteContent(tab.content);
    }
    setShowNoteList(false);
  };

  const toggleFolder = (folderPath) => {
    setExpandedFolders(prev => {
      const newSet = new Set(prev);
      if (newSet.has(folderPath)) {
        newSet.delete(folderPath);
      } else {
        newSet.add(folderPath);
      }
      return newSet;
    });
  };

  const updateNoteContent = async (newContent) => {
    setNoteContent(newContent);
    if (activeTab) {
      try {
        await invoke('write_note', { path: activeTab, content: newContent });
        // Update tab content
        setOpenTabs(prev => prev.map(tab => 
          tab.path === activeTab ? { ...tab, content: newContent } : tab
        ));
      } catch (error) {
        console.error('Failed to save note:', error);
      }
    }
  };

  const renderFileTree = (items, level = 0) => {
    return items.map((item, index) => (
      <div key={index} style={{ marginLeft: `${level * 16}px` }}>
        {item.type === 'folder' ? (
          <div>
            <div 
              className="flex items-center py-1 px-2 hover:bg-[#333] cursor-pointer"
              onClick={() => toggleFolder(item.path)}
            >
              <span className="mr-2">
                {expandedFolders.has(item.path) ? '📂' : '📁'}
              </span>
              {item.name}
            </div>
            {expandedFolders.has(item.path) && item.children && (
              <div>
                {renderFileTree(item.children, level + 1)}
              </div>
            )}
          </div>
        ) : (
          <div 
            className="flex items-center py-1 px-2 hover:bg-[#333] cursor-pointer"
            onClick={() => openNote(item.path, item.name)}
          >
            <span className="mr-2">📄</span>
            {removeMarkdownExtension(item.name)}
          </div>
        )}
      </div>
    ));
  };

  return (
    <div className="relative flex flex-col h-screen">
      <style>{`
        /* Define CSS custom properties for tab styling */
        :root {
          --tab-curve: 6px;
          --tab-outline-width: 1px;
          --tab-outline-color: #353636;
          --tab-background-active: #262626;
        }

        /* When tabs are squeezed, reduce width proportionally */
        .tabs-squeezed .tab-header {
          width: auto;
          flex: 1;
        }

        /* Pseudo-elements for curved edges */
        .tab-header::before,
        .tab-header::after {
          position: absolute;
          bottom: 0.4px;
          content: '';
          width: calc(var(--tab-curve) * 2);
          height: calc(var(--tab-curve) * 2);
          border-radius: 100%;
          pointer-events: none;
          z-index: -1;
        }

        .tab-header::before {
          left: calc(var(--tab-curve) * -2);
          clip-path: inset(50% calc(var(--tab-curve) * -1) 0 50%);
        }

        .tab-header::after {
          right: calc(var(--tab-curve) * -2);
          clip-path: inset(50% 50% 0 calc(var(--tab-curve) * -1));
        }

        /* Active tab styling with curved edges */
        .tab-header.active::before,
        .tab-header.active::after {
          box-shadow: 
            inset 0 0 0 var(--tab-outline-width) var(--tab-outline-color), 
            0 0 0 calc(var(--tab-curve) * 4) var(--tab-background-active);
        }

        /* Active tab gets elevated appearance */
        .tab-header.active {
          background-color: var(--tab-background-active);
          border-color: var(--tab-outline-color);
          z-index: 2;
           /* Extend 1px below to cover header border */
        }

        /* Ensure curved edges don't get clipped */
        .tab-header.active::before,
        .tab-header.active::after {
          z-index: -1;
        }
      `}</style>
      
      <header className="bg-[#262626] text-[#d1d1d1] h-[33px] flex border-[#353636] border-b-[0.4px] px-2">
          <div className="tabs-container mx-[6px] flex overflow-visible shrink min-w-0 mt-[6px] ">
            {openTabs.map((tab) => (
              <div 
                key={tab.path}
                className={`tab-header flex mb-[-1px] items-center  rounded-sm px-[1px] pb-[4px] border-[#353636] relative border-t border-l border-r z-10 w-[167px] min-w-[80px] shrink ${
                  activeTab === tab.path 
                    ? 'active border-[#353636]'
                    : 'border-transparent text-[#999]'
                }`}
               
              >
                <div className={`flex items-center w-full h-full relative rounded-sm px-1 group ${
                  activeTab === tab.path 
                    ? ''
                    : 'hover:bg-[#323232]'
                }`}  onClick={() => switchTab(tab.path)}>
                <span className="text-[11px] truncate flex-1 select-none">{tab.name}</span>
                <button 
  className={`ml-1 hover:text-white flex-shrink-0 ${
    activeTab === tab.path ? '' : 'hidden group-hover:block'
  }`}
  onClick={(e) => {
    e.stopPropagation();
    closeTab(tab.path);
  }}
>
  <X size={14} />
</button>
                </div>
              </div>
            ))}
          </div>
           <button 
            className="w-[22px] h-full mr-3 pt-2 pb-[6px] flex justify-center items-center text-[#888] hover:text-[#d1d1d1]"
            disabled={!vaultPath}
            onClick={() => setShowNoteList(!showNoteList)}
          >
            <Plus size={15} />
          </button>
       
       
      </header>

      <div className="main flex-1 bg-[#262626] text-[#888888]">
        {!vaultPath ? (
          <div className="flex-1 flex items-center justify-center text-[13px] flex-col">
            <div 
              className="cursor-pointer hover:text-[#d1d1d1] transition-colors"
              onClick={openVault}
            >
              Open existing vault (Ctrl + Shift + O)
            </div>
          </div>
        ) : showNoteList ? (
          <div className="flex-1 p-4 overflow-y-auto">
            <div className="text-sm mb-4 font-medium text-[#d1d1d1]">
              Files in vault
            </div>
            {renderFileTree(files)}
          </div>
        ) : activeTab ? (
          <div className="flex-1 p-4">
            <textarea
              value={noteContent}
              onChange={(e) => updateNoteContent(e.target.value)}
              className="w-full h-full bg-transparent text-[#d1d1d1] resize-none outline-none font-mono text-sm"
              placeholder="Start writing..."
            />
          </div>
        ) : (
          <div className="flex-1 flex items-center justify-center text-[13px]">
            <div>Select a note to start editing</div>
          </div>
        )}
      </div>
    </div>
  );
}

export default App;