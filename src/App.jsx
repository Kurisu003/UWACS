import { invoke } from "@tauri-apps/api/core";
import { useState } from "react";
import "./App.css";

function App() {
    const [start_status, set_start_status] = useState("");
    const [name, setName] = useState("");

    async function start(program) {
        set_start_status(await invoke("start", { program }));
    }

    async function restart(path) {
        set_start_status(await invoke("restart", { path }));
    }

    return (
        <main className="main_page_container">
            <form
                className="row"
                onSubmit={(e) => {
                    e.preventDefault();
                    greet();
                }}
            >
                {/* <input
                    id="greet-input"
                    onChange={(e) => setName(e.currentTarget.value)}
                    placeholder="Enter a name..."
                /> */}
                <h1>PFP Writer</h1>
                <button onClick={() => start("pfp_writer")}>⏻</button>
                <button onClick={() => restart("test")}>⟳</button>
                <div className="status_panel">
                    <p>HID: {`${start_status == "OK" ? "✅" : "❌"}`}</p>
                    <p>DCS Bios: </p>
                </div>
            </form>
            <p>{start_status}</p>
        </main>
    );
}

export default App;
