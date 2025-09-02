import { invoke } from "@tauri-apps/api/core";
import { useEffect, useState } from "react";
import "./App.css";

function App() {
    const [start_status, set_start_status] = useState("");
    const [name, setName] = useState("");
    const [pfp_writer_stdout, set_pfp_writer_stdout] = useState([[""], [""]]);

    async function start(program) {
        set_start_status(await invoke("start", { program }));
    }

    async function restart(path) {
        set_start_status(await invoke("restart", { path }));
    }

    async function set_pfp_data() {
        let out = await invoke("read_available", {});
        if (out[0].length > 0 || out[1].length > 0) {
            set_pfp_writer_stdout((pfp_writer_stdout) => [
                ...pfp_writer_stdout,
                out,
            ]);
            console.log(out);
        }
    }

    useEffect(() => {
        setInterval(() => {
            set_pfp_data();
        }, 1000);
    });

    return (
        <>
            <main className="main_page_container">
                <div>
                    {/* <input
                    id="greet-input"
                    onChange={(e) => setName(e.currentTarget.value)}
                    placeholder="Enter a name..."
                /> */}
                    <h1>PFP Writer</h1>
                    <button onClick={() => start("pfp_writer")}>⏻</button>
                    <button onClick={() => restart("test")}>⟳</button>
                    <button onClick={() => get_stdout()}>⟳</button>
                    <div className="status_panel">
                        <p>HID: {`${start_status == "OK" ? "✅" : "❌"}`}</p>
                        <p>DCS Bios: </p>
                    </div>
                </div>
                <p>{start_status}</p>
            </main>
            <div className="debug_console">
                {" "}
                {pfp_writer_stdout?.map((entry, i) => (
                    <div key={i}>{entry}</div>
                ))}
            </div>
        </>
    );
}

export default App;
