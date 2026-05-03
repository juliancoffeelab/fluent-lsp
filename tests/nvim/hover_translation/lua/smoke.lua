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

  local found = vim.fn.searchpos("welcome-title", "n")
  vim.api.nvim_win_set_cursor(0, { found[1], found[2] - 1 })

  local responses = vim.lsp.buf_request_sync(0, "textDocument/hover", position_params(), 3000)
  local hover = assert(responses[client_id] and responses[client_id].result, "missing plain hover")
  local value = hover.contents.value
  assert(value == "```ftl\nBienvenido\n```", "unexpected plain hover preview: " .. value)

  found = vim.fn.searchpos("Abre la build mas reciente de", "n")
  vim.api.nvim_win_set_cursor(0, { found[1], found[2] - 1 })

  responses = vim.lsp.buf_request_sync(0, "textDocument/hover", position_params(), 3000)
  hover = assert(responses[client_id] and responses[client_id].result, "missing plain value hover")
  value = hover.contents.value
  assert(value:match("^```ftl\nOpen the latest { %-brand%-name } build and pick up where you left off%.\n```\n\n---\n\n```ftl\nAbre la build mas reciente de { %-brand%-name } y sigue donde lo dejaste%.\n```$"), "missing separator-delimited plain value preview")

  found = vim.fn.searchpos("\\[female\\] ella", "n")
  vim.api.nvim_win_set_cursor(0, { found[1], found[2] - 1 })

  responses = vim.lsp.buf_request_sync(0, "textDocument/hover", position_params(), 3000)
  hover = assert(responses[client_id] and responses[client_id].result, "missing selector hover")
  value = hover.contents.value
  assert(value:match("^`%$gender=female`, `%$count=%*`\n\n```ftl\nCopy the download link for her account on { %$count } devices now%.\n```\n\n---\n\n`%$gender=female`, `%$count=%*`\n\n```ftl\nCopia el enlace de descarga para la cuenta de ella en { %$count } dispositivos ahora%.\n```$"), "missing separator-delimited selector preview")
  assert(not value:match("\\\\n"), "hover should not escape newlines")

  found = vim.fn.searchpos("Instala la build recomendada para la cuenta de", "n")
  vim.api.nvim_win_set_cursor(0, { found[1], found[2] - 1 })

  responses = vim.lsp.buf_request_sync(0, "textDocument/hover", position_params(), 3000)
  hover = assert(responses[client_id] and responses[client_id].result, "missing translated attribute hover")
  value = hover.contents.value
  assert(value:match("^`%$gender=%*`, `%$count=%*`\n\n```ftl\nInstall the recommended build for their account on { %$count } devices now%.\n```\n\n---\n\n`%$gender=%*`, `%$count=%*`\n\n```ftl\nInstala la build recomendada para la cuenta de elle en { %$count } dispositivos ahora%.\n```$"), "missing separator-delimited translated attribute preview")

  vim.api.nvim_win_set_cursor(0, { 1, 0 })
  found = vim.fn.searchpos("en { $count } { $count ->", "n")
  vim.api.nvim_win_set_cursor(0, { found[1], found[2] - 1 })

  responses = vim.lsp.buf_request_sync(0, "textDocument/hover", position_params(), 3000)
  hover = assert(responses[client_id] and responses[client_id].result, "missing post-selector hover")
  value = hover.contents.value
  assert(value:match("^`%$gender=other`, `%$count=%*`\n\n```ftl\nCopy the download link for their account on { %$count } devices now%.\n```\n\n---\n\n`%$gender=other`, `%$count=%*`\n\n```ftl\nCopia el enlace de descarga para la cuenta de elle en { %$count } dispositivos ahora%.\n```$"), "missing post-selector formatted preview")

  vim.api.nvim_win_set_cursor(0, { 1, 0 })
  found = vim.fn.searchpos("\\[one\\] dispositivo", "n")
  vim.api.nvim_win_set_cursor(0, { found[1], found[2] - 1 })

  responses = vim.lsp.buf_request_sync(0, "textDocument/hover", position_params(), 3000)
  hover = assert(responses[client_id] and responses[client_id].result, "missing second-selector hover")
  value = hover.contents.value
  assert(value:match("^`%$gender=other`, `%$count=one`\n\n```ftl\nCopy the download link for their account on { %$count } device now%.\n```\n\n---\n\n`%$gender=other`, `%$count=one`\n\n```ftl\nCopia el enlace de descarga para la cuenta de elle en { %$count } dispositivo ahora%.\n```$"), "missing concatenated selector formatted preview")

  write_result(result_path, {
    ok = true,
    feature = "hover_translation",
  })
  vim.cmd.qa()
end

return M
