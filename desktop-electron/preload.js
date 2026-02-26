const { contextBridge, ipcRenderer } = require('electron');

contextBridge.exposeInMainWorld('monitordApi', {
  startAgent: (payload) => ipcRenderer.invoke('agent:start', payload),
  stopAgent: () => ipcRenderer.invoke('agent:stop'),
  agentStatus: () => ipcRenderer.invoke('agent:status'),
  setTelegramEnabled: (enabled) => ipcRenderer.invoke('telegram:set-enabled', enabled),
  fetchState: (baseUrl) => ipcRenderer.invoke('state:fetch', baseUrl),
  readConfig: (configPath) => ipcRenderer.invoke('config:read', configPath),
  writeConfig: (payload) => ipcRenderer.invoke('config:write', payload),
  loadStructuredConfig: (configPath) => ipcRenderer.invoke('config:load-structured', configPath),
  saveStructuredConfig: (payload) => ipcRenderer.invoke('config:save-structured', payload),
});
