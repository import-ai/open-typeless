// This app uses a static frontend directory (no Vite bundler). Tauri exposes
// its API on window.__TAURI__ through `withGlobalTauri` in tauri.conf.json.
const { invoke } = window.__TAURI__.core;
const { listen } = window.__TAURI__.event;
const button=document.querySelector('#toggle'), status=document.querySelector('#status'), micDot=document.querySelector('#mic-dot'), micLabel=document.querySelector('#mic-label'); let recording=false;
const shortcutInput=document.querySelector('#shortcut'), shortcutHint=document.querySelector('#shortcut-hint');
function micState(state,label){micDot.className=`dot ${state}`;micLabel.textContent=label}
async function start(){button.disabled=true;micState('starting','麦克风启动中…');status.textContent='等待麦克风第一帧音频…';try{await invoke('start_recording'); recording=true; button.disabled=false; button.textContent='停止录音'; button.classList.add('recording'); micState('ready','麦克风已就绪'); status.textContent='录音中…'}catch(e){button.disabled=false;micState('error','麦克风未就绪');status.textContent=String(e)}}
async function stop(){try{const path=await invoke('stop_recording'); recording=false; button.textContent='处理中…'; button.disabled=true; status.textContent='正在上传和识别…'; const text=await invoke('transcribe_file',{path}); status.textContent=text||'未识别到文字';}catch(e){status.textContent=String(e)}finally{button.disabled=false;button.textContent='开始录音';button.classList.remove('recording');recording=false;micState('','麦克风未启动')}}
button.onclick=()=>recording?stop():start();
listen('recording-starting',()=>{button.disabled=true;micState('starting','麦克风启动中…');status.textContent='等待麦克风第一帧音频…'});
listen('recording-started',()=>{recording=true;button.disabled=false;button.textContent='停止录音';button.classList.add('recording');micState('ready','麦克风已就绪');status.textContent='录音中…'});
listen('recording-stopped',async e=>{recording=false;button.disabled=true;status.textContent='正在上传和识别…';try{const text=await invoke('transcribe_file',{path:e.payload});status.textContent=text||'未识别到文字'}catch(err){status.textContent=String(err)}finally{button.disabled=false;button.textContent='开始录音';button.classList.remove('recording');micState('','麦克风未启动')}});
listen('recording-error',e=>{recording=false;button.disabled=false;button.textContent='开始录音';button.classList.remove('recording');micState('error','麦克风未就绪');status.textContent=String(e.payload)});
document.querySelector('#save').onclick=async()=>{try{const value=shortcutInput.value.trim();await invoke('set_shortcut',{shortcut:value});shortcutHint.textContent=value;status.textContent='快捷键已更新'}catch(e){status.textContent=String(e)}};
invoke('get_settings').then(settings=>{shortcutInput.value=settings.shortcut;shortcutHint.textContent=settings.shortcut}).catch(e=>{status.textContent=String(e)});
