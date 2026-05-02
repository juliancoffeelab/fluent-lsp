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

  local attached = vim.wait(5000, function()
    for _, client in ipairs(vim.lsp.get_clients({ bufnr = 0 })) do
      if client.id == client_id and client.server_capabilities.hoverProvider then
        return true
      end
    end
    return false
  end, 50)
  assert(attached, "hover provider not ready")

  local found = vim.fn.searchpos("welcome-title", "n")
  vim.api.nvim_win_set_cursor(0, { found[1], found[2] - 1 })

  local responses = vim.lsp.buf_request_sync(0, "textDocument/hover", position_params(), 5000)
  local hover = assert(responses[client_id] and responses[client_id].result, "missing plain hover")
  local value = hover.contents.value
  assert(value == "```ftl\nBienvenido\n```", "unexpected plain hover preview: " .. value)

  local found = vim.fn.searchpos("\\[female\\] ella", "n")
  vim.api.nvim_win_set_cursor(0, { found[1], found[2] - 1 })

  responses = vim.lsp.buf_request_sync(0, "textDocument/hover", position_params(), 5000)
  hover = assert(responses[client_id] and responses[client_id].result, "missing selector hover")
  value = hover.contents.value
  assert(value:match("`%$gender=female`, `%$count=%*`"), "missing selected selector header")
  assert(value:match("```ftl\nCopia el enlace de descarga para la cuenta de ella en { %$count } dispositivos ahora%.\n```"), "missing formatted local selector preview")
  assert(not value:match("\\\\n"), "hover should not escape newlines")

  found = vim.fn.searchpos("en { $count } { $count ->", "n")
  vim.api.nvim_win_set_cursor(0, { found[1], found[2] - 1 })

  responses = vim.lsp.buf_request_sync(0, "textDocument/hover", position_params(), 5000)
  hover = assert(responses[client_id] and responses[client_id].result, "missing post-selector hover")
  value = hover.contents.value
  assert(value:match("`%$gender=other`, `%$count=%*`"), "missing post-selector retained header")
  assert(value:match("```ftl\nCopia el enlace de descarga para la cuenta de elle en { %$count } dispositivos ahora%.\n```"), "missing post-selector formatted preview")

  write_result(result_path, {
    ok = true,
    feature = "hover_translation",
  })
  vim.cmd.qa()
end

return M
