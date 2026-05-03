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

  vim.cmd.edit(workspace .. "/locales/es/app.ftl")
  local client_id = start_client(server, workspace)

  local attached = vim.wait(3000, function()
    for _, client in ipairs(vim.lsp.get_clients({ bufnr = 0 })) do
      if client.id == client_id and client.server_capabilities.hoverProvider then
        return true
      end
    end
    return false
  end, 50)
  assert(attached, "hover provider not ready")

  local found = vim.fn.searchpos("\\[female\\] ella misma", "n")
  vim.api.nvim_win_set_cursor(0, { found[1], found[2] - 1 })

  local responses = vim.lsp.buf_request_sync(0, "textDocument/hover", position_params(), 3000)
  local hover = assert(responses[client_id] and responses[client_id].result, "missing mismatch hover")
  local value = hover.contents.value
  assert(value:match("^`%$platform=%*`, `%$count=%*`\n\n```ftl\nSummary for mobile users with { %$count } packages ready%.\n```\n\n---\n\n`%$gender=female`, `%$count=%*`\n\n```ftl\nResumen para ella misma con { %$count } paquetes listo%.\n```$"), "missing mismatch selector hover")

  vim.api.nvim_win_set_cursor(0, { 1, 0 })
  found = vim.fn.searchpos("\\[0\\] ningun paquete", "n")
  vim.api.nvim_win_set_cursor(0, { found[1], found[2] - 1 })

  responses = vim.lsp.buf_request_sync(0, "textDocument/hover", position_params(), 3000)
  hover = assert(responses[client_id] and responses[client_id].result, "missing zero-count hover")
  value = hover.contents.value
  assert(value:match("^`%$platform=%*`, `%$count=0`\n\n```ftl\nSummary for mobile users with no packages ready%.\n```\n\n---\n\n`%$gender=other`, `%$count=0`\n\n```ftl\nResumen para elle misme con ningun paquete listo%.\n```$"), "missing zero-count selector hover")

  vim.api.nvim_win_set_cursor(0, { 1, 0 })
  found = vim.fn.searchpos("\\[1\\] un paquete", "n")
  vim.api.nvim_win_set_cursor(0, { found[1], found[2] - 1 })

  responses = vim.lsp.buf_request_sync(0, "textDocument/hover", position_params(), 3000)
  hover = assert(responses[client_id] and responses[client_id].result, "missing one-count hover")
  value = hover.contents.value
  assert(value:match("^`%$platform=%*`, `%$count=1`\n\n```ftl\nSummary for mobile users with one package ready%.\n```\n\n---\n\n`%$gender=other`, `%$count=1`\n\n```ftl\nResumen para elle misme con un paquete listo%.\n```$"), "missing one-count selector hover")

  write_result(result_path, {
    ok = true,
    feature = "hover_selector_mismatch",
  })
  vim.cmd.qa()
end

return M
