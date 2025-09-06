import { invoke } from "@tauri-apps/api/core";
import { useEffect, useState } from "react";
import "./App.css";

function App() {
    const [pfp_hid_status, set_pfp_hid_status] = useState("❔");
    const [ufc_hid_status, set_ufc_hid_status] = useState("❔");
    const [dcs_bios_status, set_dcs_bios_status] = useState("❔");
    const [dcs_module, set_dcs_module] = useState("");
    const [writer_stdout, set_writer_stdout] = useState([[""], [""]]);

    async function start(program) {
        if (program == "pfp_writer") set_pfp_hid_status("❔");
        else if (program == "ufc_writer") set_ufc_hid_status("❔");

        set_dcs_bios_status("❔");
        await invoke("start", { program });
    }

    async function set_stdout_data() {
        let out = await invoke("read_available", {});
        // assume DCS Bios is ✅ and replace it with ❌
        // if something goes wrong
        set_dcs_module("To be implemented");
        if (out[0].length > 0 || out[1].length > 0) {
            for (let i = 0; i <= 1; i++) {
                let latest_line = String(out[i]);
                // PFP HID FALSE
                if (latest_line.includes("Error: PFP_WRITER: No HID")) {
                    set_pfp_hid_status("❌");
                }
                // UFC HID FALSE
                else if (latest_line.includes("Error: UFC_WRITER: No HID")) {
                    set_ufc_hid_status("❌");
                }
                // PFP HID TRUE
                else if (
                    latest_line.includes("PFP_WRITER: Sent init packets")
                ) {
                    set_pfp_hid_status("✅");
                }
                // UFC HID TRUE
                else if (latest_line.includes("UFC_WRITER: HID device OK")) {
                    set_ufc_hid_status("✅");
                }
                // DCS BIOS FALSE
                else if (latest_line.includes("read_stream error")) {
                    set_dcs_bios_status("❌");
                }
                // DCS BIOS FALSE
                else if (latest_line.includes("Connection closed")) {
                    set_dcs_bios_status("❌");
                }
                // DCS BIOS TRUE
                if (latest_line.includes("Connection OK")) {
                    console.log("A");
                    set_dcs_bios_status("✅");
                }
            }
            // if () {
            set_writer_stdout((pfp_writer_stdout) => [
                ...pfp_writer_stdout,
                out,
            ]);
            // }
        }
    }

    function clear_console() {
        set_writer_stdout(() => []);
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
                <p>Module: {dcs_module}</p>
                <div className="program_wrapper">
                    <h1>PFP Writer</h1>
                    <button onClick={() => start("pfp_writer")}>⏻</button>
                    <div className="status_panel">
                        <p>Device connection:{pfp_hid_status}</p>
                        <p>DCS Bios: {dcs_bios_status}</p>
                    </div>
                </div>
                <div className="program_wrapper">
                    <h1>UFC Writer</h1>
                    <button onClick={() => start("ufc_writer")}>⏻</button>
                    <div className="status_panel">
                        <p>Device connection:{ufc_hid_status}</p>
                        <p>DCS Bios: {dcs_bios_status}</p>
                    </div>
                </div>
            </div>
            <div className="debug_console_container">
                <div className="debug_console">
                    {writer_stdout?.map((entry, i) => (
                        <div key={"stout_" + i}>{entry}</div>
                    ))}
                    <p>.</p>
                </div>
                <button onClick={() => clear_console()}>
                    MASTER<br></br>CAUTION
                </button>
            </div>
        </>
    );
}

export default App;
