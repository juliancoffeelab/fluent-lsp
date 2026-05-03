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

function M.run()
  local workspace = assert(vim.env.FLUENT_LSP_WORKSPACE ~= "" and vim.env.FLUENT_LSP_WORKSPACE)
  local server = assert(vim.env.FLUENT_LSP_BIN ~= "" and vim.env.FLUENT_LSP_BIN)
  local result_path = assert(vim.env.FLUENT_LSP_RESULT ~= "" and vim.env.FLUENT_LSP_RESULT)

  vim.cmd.edit(workspace .. "/locales/es/app.ftl")
  start_client(server, workspace)

  local attached = vim.wait(5000, function()
    for _, client in ipairs(vim.lsp.get_clients({ bufnr = 0 })) do
      if client.name == "fluent-lsp" then
        return true
      end
    end
    return false
  end, 50)
  assert(attached, "LSP client did not initialize")
  local client = vim.lsp.get_clients({ bufnr = 0, name = "fluent-lsp" })[1]
  assert(client, "missing fluent-lsp client")

  assert(client.server_capabilities.definitionProvider, "missing definitionProvider")
  assert(client.server_capabilities.hoverProvider, "missing hoverProvider")
  assert(client.server_capabilities.inlayHintProvider == nil, "unexpected inlayHintProvider")
  assert(client.server_capabilities.referencesProvider, "missing referencesProvider")
  assert(client.server_capabilities.codeLensProvider, "missing codeLensProvider")
  assert(client.server_capabilities.executeCommandProvider, "missing executeCommandProvider")

  write_result(result_path, {
    ok = true,
    feature = "minimal_server_attach",
  })
  vim.cmd.qa()
end

return M
