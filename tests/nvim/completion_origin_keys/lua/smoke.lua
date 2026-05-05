local M = {}

local function write_result(result_path, payload)
  vim.fn.writefile({ vim.json.encode(payload) }, result_path)
end

local function start_client(server, workspace)
  local client_id = vim.lsp.start({
    name = "fluent-lsp",
    cmd = { server },
    root_dir = workspace,
  })
  assert(client_id, "failed to start fluent-lsp")
  return client_id
end

local function wait_for_client(client_id, capability)
  local ready = vim.wait(3000, function()
    for _, client in ipairs(vim.lsp.get_clients({ bufnr = 0 })) do
      if client.id == client_id and client.server_capabilities[capability] then
        return true
      end
    end
    return false
  end, 50)
  assert(ready, capability .. " not ready")
end

local function stop_client(client_id)
  vim.lsp.stop_client(client_id, true)
  vim.wait(3000, function()
    for _, client in ipairs(vim.lsp.get_clients()) do
      if client.id == client_id then
        return false
      end
    end
    return true
  end, 50)
end

function M.run()
  local workspace = assert(vim.env.FLUENT_LSP_WORKSPACE ~= "" and vim.env.FLUENT_LSP_WORKSPACE)
  local server = assert(vim.env.FLUENT_LSP_BIN ~= "" and vim.env.FLUENT_LSP_BIN)
  local result_path = assert(vim.env.FLUENT_LSP_RESULT ~= "" and vim.env.FLUENT_LSP_RESULT)
  local files = {
    workspace .. "/locales/es/app_download.ftl",
    workspace .. "/locales/es/app_commented.ftl",
    workspace .. "/locales/es/dialogs/menu_bare_dot.ftl",
    workspace .. "/locales/es/dialogs/menu_label_prefix.ftl",
  }

  vim.cmd.edit(files[1])
  local client_id = start_client(server, workspace)
  wait_for_client(client_id, "completionProvider")

  for index = 2, #files do
    vim.cmd.edit(files[index])
  end

  stop_client(client_id)
  write_result(result_path, {
    ok = true,
    feature = "completion_origin_keys",
  })
  vim.cmd("qa!")
end

return M
