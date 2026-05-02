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

local function run_selector_lens(client_id, source_buf, needle, filename_pattern, expected_text)
  vim.api.nvim_set_current_buf(source_buf)
  local found = vim.fn.searchpos(needle, "n")
  assert(found[1] > 0, "missing selector target: " .. needle)
  vim.api.nvim_win_set_cursor(0, { found[1], found[2] - 1 })
  vim.lsp.codelens.run({ client_id = client_id })

  local shown = vim.wait(5000, function()
    local current = vim.api.nvim_buf_get_name(0)
    local filename = vim.fn.fnamemodify(current, ":t")
    if not filename:match(filename_pattern) then
      return false
    end
    local text = table.concat(vim.api.nvim_buf_get_lines(0, 0, -1, false), "\n")
    return text:match(expected_text) ~= nil
  end, 50)
  assert(shown, "codelens did not open expected selector combinations document for " .. needle)
end

function M.run()
  local workspace = assert(vim.env.FLUENT_LSP_WORKSPACE ~= "" and vim.env.FLUENT_LSP_WORKSPACE)
  local server = assert(vim.env.FLUENT_LSP_BIN ~= "" and vim.env.FLUENT_LSP_BIN)
  local result_path = assert(vim.env.FLUENT_LSP_RESULT ~= "" and vim.env.FLUENT_LSP_RESULT)

  vim.cmd.edit(workspace .. "/locales/es/app.ftl")
  local client_id = start_client(server, workspace)

  local attached = vim.wait(5000, function()
    for _, client in ipairs(vim.lsp.get_clients({ bufnr = 0 })) do
      if client.id == client_id and client.server_capabilities.codeLensProvider then
        return true
      end
    end
    return false
  end, 50)
  assert(attached, "codelens provider not ready")

  vim.lsp.codelens.enable(true, { bufnr = 0 })
  local ready = vim.wait(5000, function()
    return #vim.lsp.codelens.get({ bufnr = 0, client_id = client_id }) > 0
  end, 50)
  assert(ready, "codelens did not populate")

  local lenses = vim.lsp.codelens.get({ bufnr = 0, client_id = client_id })
  assert(#lenses >= 3, "expected selector codelenses for message and attribute values")
  local source_buf = vim.api.nvim_get_current_buf()

  run_selector_lens(
    client_id,
    source_buf,
    "install-hint",
    "^fluent%-lsp%-selector%-combinations%-.+%-install%-hint%.md$",
    "Presiona Ctrl %+ C para copiar el enlace de descarga ahora%."
  )
  run_selector_lens(
    client_id,
    source_buf,
    "tooltip =",
    "^fluent%-lsp%-selector%-combinations%-.+%-download%-action_tooltip%.md$",
    "Instala la build de escritorio mas reciente ahora%."
  )

  write_result(result_path, {
    ok = true,
    feature = "codelens_selector_combinations",
  })
  vim.cmd.qa()
end

return M
