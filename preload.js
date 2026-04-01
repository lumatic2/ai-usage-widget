const { contextBridge, ipcRenderer } = require('electron');

contextBridge.exposeInMainWorld('codexWidget', {
  getInitialState: () => ipcRenderer.invoke('widget:get-initial-state'),
  onState: (callback) => {
    const listener = (_event, payload) => callback(payload);
    ipcRenderer.on('widget-state', listener);
    return () => ipcRenderer.removeListener('widget-state', listener);
  },
  hide: () => ipcRenderer.send('widget:hide')
});
