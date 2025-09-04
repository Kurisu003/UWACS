import { invoke } from "@tauri-apps/api/core";
import { useEffect, useState } from "react";
import "./App.css";

function App() {
    const [pfp_hid_status, set_pfp_hid_status] = useState(false);
    const [ufc_hid_status, set_ufc_hid_status] = useState(false);
    const [dcs_bios_status, set_dcs_bios_status] = useState(false);
    const [dcs_module, set_dcs_module] = useState("");
    const [writer_stdout, set_writer_stdout] = useState([[""], [""]]);

    async function start(program) {
        await invoke("start", { program });
    }

    async function set_stdout_data() {
        let out = await invoke("read_available", {});
        if (out[0].length > 0 || out[1].length > 0) {
            for (let i = 0; i <= 1; i++) {
                let latest_line = String(out[i]);
                // PFP HID FALSE
                if (latest_line.startsWith("Error: PFP_WRITER: No HID")) {
                    set_pfp_hid_status(false);
                }
                // UFC HID FALSE
                else if (latest_line.startsWith("Error: UFC_WRITER: No HID")) {
                    set_ufc_hid_status(false);
                }
                // PFP HID TRUE
                else if (
                    latest_line.startsWith("PFP_WRITER: Sent init packets")
                ) {
                    set_pfp_hid_status(true);
                }
                // UFC HID TRUE
                else if (latest_line.startsWith("UFC_WRITER: HID device OK")) {
                    set_ufc_hid_status(true);
                }
                // DCS BIOS FALSE
                else if (latest_line.startsWith("read_stream error")) {
                    set_dcs_bios_status(false);
                }
                // DCS BIOS TRUE
                else if (latest_line.startsWith("Module: ")) {
                    set_dcs_bios_status(true);
                    set_dcs_module("A");
                }
            }
            if (!latest_line.startsWith("Module")) {
                set_writer_stdout((pfp_writer_stdout) => [
                    ...pfp_writer_stdout,
                    out,
                ]);
            }
        }
    }

    useEffect(() => {
        const interval = setInterval(() => {
            set_stdout_data();
        }, 250);

        return () => clearInterval(interval);
    }, []);

    return (
        <>
            <div className="main_page_container">
                <div className="program_wrapper">
                    <h1>PFP Writer</h1>
                    <button onClick={() => start("pfp_writer")}>⏻</button>
                    <div className="status_panel">
                        <p>HID: {`${pfp_hid_status ? "✅" : "❌"}`}</p>
                        <p>DCS Bios: {`${dcs_bios_status ? "✅" : "❌"}`}</p>
                    </div>
                </div>
                <div className="program_wrapper">
                    <h1>UFC Writer</h1>
                    <button onClick={() => start("ufc_writer")}>⏻</button>
                    <div className="status_panel">
                        <p>HID: {`${ufc_hid_status ? "✅" : "❌"}`}</p>
                        <p>DCS Bios: {`${dcs_bios_status ? "✅" : "❌"}`}</p>
                    </div>
                </div>
            </div>
            <div className="debug_console">
                {" "}
                <p>.</p>
                {writer_stdout?.map((entry, i) => (
                    <div key={"stout_" + i}>{entry}</div>
                ))}
            </div>
        </>
    );
}

export default App;
