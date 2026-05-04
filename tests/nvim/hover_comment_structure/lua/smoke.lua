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

local function request_hover(client_id)
  local responses = vim.lsp.buf_request_sync(0, "textDocument/hover", position_params(), 3000)
  local hover = assert(responses[client_id] and responses[client_id].result, "missing hover")
  return hover.contents.value
end

function M.run()
  local workspace = assert(vim.env.FLUENT_LSP_WORKSPACE ~= "" and vim.env.FLUENT_LSP_WORKSPACE)
  local server = assert(vim.env.FLUENT_LSP_BIN ~= "" and vim.env.FLUENT_LSP_BIN)
  local result_path = assert(vim.env.FLUENT_LSP_RESULT ~= "" and vim.env.FLUENT_LSP_RESULT)

  vim.cmd.edit(workspace .. "/locales/en/app.ftl")
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

  local found = vim.fn.searchpos("commented-preview", "n")
  vim.api.nvim_win_set_cursor(0, { found[1], found[2] - 1 })

  local key_hover = request_hover(client_id)
  assert(
    key_hover == "```ftl\n# Comment-only hover coverage\n# Keep this translator guidance visible on key hover\n```",
    "unexpected key hover comments: " .. key_hover
  )

  found = vim.fn.searchpos("Preview text for hover comments", "n")
  vim.api.nvim_win_set_cursor(0, { found[1], found[2] - 1 })

  local body_hover = request_hover(client_id)
  assert(body_hover == "```ftl\nPreview text for hover comments.\n```", "unexpected body hover preview: " .. body_hover)
  assert(not body_hover:match("\n\n---\n\n"), "origin body hover should not duplicate source/current sections")
  assert(not body_hover:match("\\\\n"), "hover should not escape newlines")

  write_result(result_path, {
    ok = true,
    feature = "hover_comment_structure",
    hovers = {
      key = key_hover,
      body = body_hover,
    },
  })
  vim.cmd.qa()
end

return M
