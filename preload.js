const { contextBridge, ipcRenderer } = require('electron');

contextBridge.exposeInMainWorld('codexWidget', {
  getInitialState: () => ipcRenderer.invoke('widget:get-initial-state'),
  getSettings: () => ipcRenderer.invoke('widget:get-settings'),
  updateSettings: (partial) => ipcRenderer.invoke('widget:update-settings', partial),
  setDisplayMode: (mode) => ipcRenderer.invoke('widget:set-display-mode', mode),
  refreshNow: () => ipcRenderer.invoke('widget:refresh-now'),
  onState: (callback) => {
    const listener = (_event, payload) => callback(payload);
    ipcRenderer.on('widget-state', listener);
    return () => ipcRenderer.removeListener('widget-state', listener);
  },
  hide: () => ipcRenderer.send('widget:hide')
});
