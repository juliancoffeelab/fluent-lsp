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
  assert(responses[client_id], "missing hover response")
  return responses[client_id].result
end

local function wait_for_hover(client_id)
  local hover
  local ready = vim.wait(3000, function()
    hover = request_hover(client_id)
    return hover ~= nil and hover ~= vim.NIL
  end, 50)
  assert(ready, "hover result not ready")
  return hover
end

function M.run()
  local workspace = assert(vim.env.FLUENT_LSP_WORKSPACE ~= "" and vim.env.FLUENT_LSP_WORKSPACE)
  local server = assert(vim.env.FLUENT_LSP_BIN ~= "" and vim.env.FLUENT_LSP_BIN)
  local result_path = assert(vim.env.FLUENT_LSP_RESULT ~= "" and vim.env.FLUENT_LSP_RESULT)

  vim.cmd.edit(workspace .. "/locales/en/dialogs/menu.ftl")
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

  local found = vim.fn.searchpos("label = Save copy", "n")
  vim.api.nvim_win_set_cursor(0, { found[1], found[2] - 1 })

  local key_hover = wait_for_hover(client_id)
  assert(
    key_hover and key_hover.contents.value == "```ftl\n# Attribute hover comment coverage\n# Keep this menu note visible on attribute key hover\n```",
    "unexpected origin key hover comments: " .. vim.inspect(key_hover)
  )

  found = vim.fn.searchpos("Save a copy before closing the dialog", "n")
  vim.api.nvim_win_set_cursor(0, { found[1], found[2] - 1 })

  local body_hover = wait_for_hover(client_id)
  assert(
    body_hover and body_hover.contents.value == "```ftl\nSave a copy before closing the dialog\n```",
    "unexpected origin body hover preview: " .. vim.inspect(body_hover)
  )
  assert(not body_hover.contents.value:match("\n\n---\n\n"), "origin hover should not duplicate source/current sections")

  found = vim.fn.searchpos("plain-menu =", "n")
  vim.api.nvim_win_set_cursor(0, { found[1], found[2] - 1 })

  local uncommented_key_hover = request_hover(client_id)
  assert(uncommented_key_hover == vim.NIL or uncommented_key_hover == nil, "uncommented key hover should be null")

  found = vim.fn.searchpos("Save the latest draft without opening the dialog", "n")
  vim.api.nvim_win_set_cursor(0, { found[1], found[2] - 1 })

  local uncommented_body_hover = wait_for_hover(client_id)
  assert(
    uncommented_body_hover and uncommented_body_hover.contents.value == "```ftl\nSave the latest draft without opening the dialog\n```",
    "unexpected uncommented body hover preview: " .. vim.inspect(uncommented_body_hover)
  )

  write_result(result_path, {
    ok = true,
    feature = "hover_origin_language",
    hovers = {
      key = key_hover.contents.value,
      body = body_hover.contents.value,
      uncommented_key = uncommented_key_hover,
      uncommented_body = uncommented_body_hover.contents.value,
    },
  })
  vim.cmd.qa()
end

return M
