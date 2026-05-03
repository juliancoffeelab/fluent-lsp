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

local function position_params()
  local cursor = vim.api.nvim_win_get_cursor(0)
  return {
    textDocument = { uri = vim.uri_from_bufnr(0) },
    position = { line = cursor[1] - 1, character = cursor[2] },
  }
end

function M.run()
  local workspace = assert(vim.env.FLUENT_LSP_WORKSPACE ~= "" and vim.env.FLUENT_LSP_WORKSPACE)
  local server = assert(vim.env.FLUENT_LSP_BIN ~= "" and vim.env.FLUENT_LSP_BIN)
  local result_path = assert(vim.env.FLUENT_LSP_RESULT ~= "" and vim.env.FLUENT_LSP_RESULT)

  vim.cmd.edit(workspace .. "/locales/en/app.ftl")
  local client_id = start_client(server, workspace)

  local attached = vim.wait(3000, function()
    for _, client in ipairs(vim.lsp.get_clients({ bufnr = 0 })) do
      if client.id == client_id and client.server_capabilities.referencesProvider then
        return true
      end
    end
    return false
  end, 50)
  assert(attached, "references provider not ready")

  local found = vim.fn.searchpos("welcome-title", "n")
  vim.api.nvim_win_set_cursor(0, { found[1], found[2] - 1 })

  local params = position_params()
  params.context = { includeDeclaration = false }
  local responses = vim.lsp.buf_request_sync(0, "textDocument/references", params, 3000)
  local items = assert(responses[client_id] and responses[client_id].result, "missing references")
  assert(#items == 2, "expected two references")

  local uris = {}
  for _, item in ipairs(items) do
    table.insert(uris, item.uri)
  end
  table.sort(uris)
  assert(uris[1]:match("locales/es/app%.ftl$"), "missing es reference")
  assert(uris[2]:match("locales/fr/app%.ftl$"), "missing fr reference")

  write_result(result_path, {
    ok = true,
    feature = "references_origin",
  })
  vim.cmd.qa()
end

return M
