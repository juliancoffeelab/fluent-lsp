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

  vim.cmd.edit(workspace .. "/locales/es/dialogs/menu.ftl")
  start_client(server, workspace)

  local attached = vim.wait(5000, function()
    for _, client in ipairs(vim.lsp.get_clients({ bufnr = 0 })) do
      if client.name == "fluent-lsp" and client.server_capabilities.definitionProvider then
        return true
      end
    end
    return false
  end, 50)
  assert(attached, "definition provider not ready")

  local found = vim.fn.searchpos("menu-save", "n")
  vim.api.nvim_win_set_cursor(0, { found[1], found[2] - 1 })
  vim.lsp.buf.definition()

  local jumped = vim.wait(5000, function()
    return vim.api.nvim_buf_get_name(0):match("locales/en/dialogs/menu%.ftl$") ~= nil
  end, 50)
  assert(jumped, "definition did not jump to nested origin file")
  assert(vim.api.nvim_win_get_cursor(0)[1] == 4, "unexpected nested definition line")

  write_result(result_path, {
    ok = true,
    feature = "workspace_locale_tree",
  })
  vim.cmd.qa()
end

return M
