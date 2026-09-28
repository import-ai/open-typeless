// This app uses a static frontend directory (no Vite bundler). Tauri exposes
// its API on window.__TAURI__ through `withGlobalTauri` in tauri.conf.json.
const { invoke } = window.__TAURI__.core;
const { listen } = window.__TAURI__.event;
const button=document.querySelector('#toggle'), status=document.querySelector('#status'); let recording=false;
async function start(){try{await invoke('start_recording'); recording=true; button.textContent='停止录音'; button.classList.add('recording'); status.textContent='录音中…'}catch(e){status.textContent=String(e)}}
async function stop(){try{const path=await invoke('stop_recording'); recording=false; button.textContent='处理中…'; button.disabled=true; status.textContent='正在上传和识别…'; const text=await invoke('transcribe_file',{path}); status.textContent=text||'未识别到文字';}catch(e){status.textContent=String(e)}finally{button.disabled=false;button.textContent='开始录音';button.classList.remove('recording');recording=false}}
button.onclick=()=>recording?stop():start();
listen('recording-started',()=>{recording=true;button.textContent='停止录音';button.classList.add('recording');status.textContent='录音中…'});
listen('recording-stopped',async e=>{recording=false;button.disabled=true;status.textContent='正在上传和识别…';try{const text=await invoke('transcribe_file',{path:e.payload});status.textContent=text||'未识别到文字'}catch(err){status.textContent=String(err)}finally{button.disabled=false;button.textContent='开始录音';button.classList.remove('recording')}});
listen('recording-error',e=>{recording=false;button.disabled=false;button.textContent='开始录音';button.classList.remove('recording');status.textContent=String(e.payload)});
