#!/usr/bin/env node

import { Command } from "commander";
import fs from "fs";
import process from "process";

// Initialize a new command program
const program = new Command();

/* key contraints */
const Keys = {
    // We use ESCAPE to switch from INSERT to NORMAL mode
    ESCAPE: "\x1b",
    COLON: ":",
    ENTER: "\r",
    BACKSPACE: "\x7f",
    ARROW_UP: "\x1b[A",
    ARROW_DOWN: "\x1b[B",
    ARROW_RIGHT: "\x1b[C",
    ARROW_LEFT: "\x1b[D",
};

const Terminal = {
    clearScreen: () => {
        process.stdout.write("\x1b[2J\x1b[H");
    },
    moveCursor: (row: number, col: number) => {
        process.stdout.write(`\x1b[${row + 1};${col + 1}H`);
    },
    invertColors: () => {
        process.stdout.write("\x1b[7m");
    },
    resetColors: () => {
        process.stdout.write("\x1b[m");
    },
};

/* Editor state */
const Editor = {
    filePath: "",
    lines: [] as string[],
    cursorX: 0,
    cursorY: 0,
    screenRows: process.stdout.rows,
    screenCols: process.stdout.columns,
    //we start with a NORMAL mode(typical vim feature)
    mode: "NORMAL",
    commandString: "",
    status: "HELP: Press `i` for insert | `ESC` to exit | `:w` to save | `:wq` to save & exit | `:q` to exit",
};

function render() {
    Terminal.clearScreen();

    const textAreaRows = Editor.screenRows - 2
    for (let i = 0; i < textAreaRows; i++) {
        Terminal.moveCursor(i, 0);
        //clearing the physical line first
        process.stdout.write("\x1b[2k")
        if (i < Editor.lines.length) {
            process.stdout.write(Editor.lines[i]);
        } else {
            process.stdout.write("~");
        }
    }

    //2. Draw the status bar
    const statusRow = Editor.screenRows - 2;
    Terminal.moveCursor(statusRow, 0)
    Terminal.invertColors();
    let statusBar: string;
    if (Editor.mode == "COMMAND") {
        statusBar = `:${Editor.commandString}`;
    } else {
        const modeIndicator = `-- ${Editor.mode} --`;
        const position = `${Editor.cursorY + 1}:${Editor.cursorX + 1}`;
        statusBar = `${modeIndicator} | ${position} | ${Editor.filePath}`;
    }
    process.stdout.write(statusBar.padEnd(Editor.screenCols));
    Terminal.resetColors();

    //3. status message line (if not in command mode)
    const messageRow = Editor.screenRows - 1;
    Terminal.moveCursor(messageRow, 0);
    process.stdout.write("\x1b[2k") //clear line
    if (Editor.mode !== "COMMAND") {
        process.stdout.write(Editor.status.slice(0, Editor.screenCols));
    }
    else{
        process.stdout.write(Editor.status.slice(0, Editor.screenCols))
    }

    //4. move cursor to its correct position
    // if in command mode, cursor moves to the status bar, otherwise to text area
    if (Editor.mode === "COMMAND") {
        Terminal.moveCursor(statusRow, (Editor.commandString.length+1))
    } else {
        const lineLen = (Editor.lines[Editor.cursorY] || "").length
        if(Editor.cursorX > lineLen)Editor.cursorX = lineLen
        Terminal.moveCursor(Editor.cursorY, Editor.cursorX);
    }
}

function saveFile() {
    try {
        fs.writeFileSync(Editor.filePath, Editor.lines.join("\n"));
        Editor.status = `File saved successfully! (${Editor.lines.length} lines)`;
    } catch (error) {
        Editor.status = `Error saving file: ${error}`;
    }
}

function handleNormalModeKeypress(key: string) {
    switch (key) {
        case "i":
            Editor.mode = "INSERT";
            Editor.status = "HELP: press `ESC` to return to NORMAL mode. ";
            break;

        case Keys.COLON:
            Editor.mode = "COMMAND";
            Editor.commandString = "";
            break;

        // navigation using h,j,k,l
        case "k":
        case Keys.ARROW_UP:
            if (Editor.cursorY > 0) Editor.cursorY--;
            break;

        case "j":
        case Keys.ARROW_DOWN:
            if (Editor.cursorY < Editor.lines.length - 1) Editor.cursorY++;
            break;

        case "h":
        case Keys.ARROW_LEFT:
            if (Editor.cursorX > 0) Editor.cursorX--;
            break;

        case "l":
        case Keys.ARROW_RIGHT:
            if (Editor.cursorX < Editor.lines[Editor.cursorY].length)
                Editor.cursorX++;
            break;

        default:
            break;
    }
}

function handleInsertModeKeypress(key: string) {
    switch (key) {
        case Keys.ESCAPE:
            Editor.mode = "NORMAL";
            Editor.status =
                "HELP: Press `i` to insert | `ESC` to exit | `:w` to save | `:q` to quit";

            if (
                Editor.cursorX > 0 &&
                Editor.cursorX >= Editor.lines[Editor.cursorY].length
            ) {
                Editor.cursorX = Math.max(
                    0,
                    Editor.lines[Editor.cursorY].length - 1
                );
            }
            break;

        case Keys.ENTER:
            // we are storing the entire current line in a variable
            const currentLine = Editor.lines[Editor.cursorY];

            /* it extracts all the content from the current cursor position to end of the line(it will be the content of the new line)
             */
            const newLine = currentLine.substring(Editor.cursorX);

            // it contain only text that appears before the cursor position
            Editor.lines[Editor.cursorY] = currentLine.substring(
                0,
                Editor.cursorX
            );

            // it inserts the extracted content as a new line in the Editor.lines array immediately after the current line.
            Editor.lines.splice(Editor.cursorY + 1, 0, newLine);

            // vertical line increase
            Editor.cursorY++;

            // place cursor at start of the line
            Editor.cursorX = 0;
            break;

        case Keys.BACKSPACE:
            if (Editor.cursorX > 0) {
                let line = Editor.lines[Editor.cursorY];

                Editor.lines[Editor.cursorY] =
                    line.slice(0, Editor.cursorX - 1) +
                    line.slice(Editor.cursorX);

                Editor.cursorX--;
            } else if (Editor.cursorY > 0) {
                const previousLine = Editor.lines[Editor.cursorY - 1];
                Editor.cursorX = previousLine.length;
                Editor.lines[Editor.cursorY - 1] =
                    previousLine +
                    Editor.lines[Editor.cursorY];
                Editor.lines.splice(Editor.cursorY, 1);
                Editor.cursorY--;
            }
            break;

        default: 
            /* if the length of the key pressed is 1 and
            not a control character like NULL, BEL, Backspace, Tab, Newline, carriage return, ESC */ 
            // if basically allows only printable characters and skip all the control keys
            if (key.length === 1 && !/[\x00-\x1F]/.test(key)) {
                let line = Editor.lines[Editor.cursorY] || "";
                Editor.lines[Editor.cursorY] =line.slice(0, Editor.cursorX) +key +line.slice(Editor.cursorX);
                Editor.cursorX++;
            }
            break;
    }
}

function handleCommandModeKeyPress(key: string) {
    switch (key) {
        case Keys.ESCAPE:
            Editor.mode = "NORMAL";
            Editor.commandString = "";
            break;

        case Keys.ENTER:
            // Executing the command
            switch (Editor.commandString) {
                case "w":
                    saveFile();
                    break;

                case "q!":
                    cleanUpAndExit();
                    break;

                case "q":
                    cleanUpAndExit();
                    break;                

                case "wq":
                    saveFile();
                    cleanUpAndExit();
                    break;

                default:
                    Editor.status = `Not an editor command: ${Editor.commandString}`;
            }

            //returning to the normal mode after executing the command
            Editor.mode = "NORMAL";
            Editor.commandString = "";
            break;

        case Keys.BACKSPACE:
            Editor.commandString = Editor.commandString.slice(0, -1);
            break;

        default:
            //Appending typed characters to the command string
            if (key.length === 1 && !/[\x00-\x1F]/.test(key)) {
                Editor.commandString += key;
            }
            break;
    }
}

function cleanUpAndExit() {
    Terminal.clearScreen()
    process.stdin.setRawMode(false)
    process.exit()
}

function handleKeypress(key: string) {
    if (Editor.mode == "NORMAL") {
        handleNormalModeKeypress(key);
    } else if (Editor.mode == "INSERT") {
        handleInsertModeKeypress(key);
    } else {
        handleCommandModeKeyPress(key);
    }
    render();
}

/* setup and initialization part */

/*  1. Read the file
    2. Store and insert that into editor's current state (Editor.lines)
*/
function runEditor(filePath: string) {
    Editor.filePath = filePath;
    try {
        const fileContent = fs.readFileSync(filePath, "utf8");
        Editor.lines = fileContent.split("\n");
    } catch (error) {
        Editor.lines = [""];
    }

    process.stdin.setRawMode(true);
    process.stdin.setEncoding("utf-8");
    process.stdin.on("data", handleKeypress);

    render();
}

program
    .name("as-nano")
    .description("CLI text-editor like nano")
    .version("1.0.0");

program
    .argument("<filename>", "The file to open or create")
    .action((filename) => {
        runEditor(filename);
    });

program.parse(process.argv);