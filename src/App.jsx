import { getCurrentWindow } from "@tauri-apps/api/window";
import { invoke } from "@tauri-apps/api/core";
import { useEffect } from "react";

function App() {

  useEffect(() => {getCurrentWindow().show();}, []);

  return (
    <div className="relative flex flex-col h-screen bg-[#262626] justify-end">
      
    </div>
  );
}

export default App;