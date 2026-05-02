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

local function range_params(start_line, start_character, end_line, end_character)
  return {
    textDocument = { uri = vim.uri_from_bufnr(0) },
    range = {
      start = { line = start_line or 0, character = start_character or 0 },
      ["end"] = { line = end_line or 40, character = end_character or 0 },
    },
  }
end

local function rendered_hints_by_label()
  local rendered_hints = vim.lsp.inlay_hint.get({ bufnr = 0 })
  local rendered_labels = {}
  local rendered_positions = {}
  for _, item in ipairs(rendered_hints) do
    local hint = item.inlay_hint
    if type(hint.label) == "string" then
      table.insert(rendered_labels, hint.label)
      rendered_positions[hint.label] = hint.position
    end
  end
  return table.concat(rendered_labels, "\n"), rendered_positions
end

function M.run()
  local workspace = assert(vim.env.FLUENT_LSP_WORKSPACE ~= "" and vim.env.FLUENT_LSP_WORKSPACE)
  local server = assert(vim.env.FLUENT_LSP_BIN ~= "" and vim.env.FLUENT_LSP_BIN)
  local result_path = assert(vim.env.FLUENT_LSP_RESULT ~= "" and vim.env.FLUENT_LSP_RESULT)

  vim.cmd.edit(workspace .. "/locales/es/app.ftl")
  local client_id = start_client(server, workspace)

  local attached = vim.wait(5000, function()
    for _, client in ipairs(vim.lsp.get_clients({ bufnr = 0 })) do
      if client.id == client_id and client.server_capabilities.inlayHintProvider then
        return true
      end
    end
    return false
  end, 50)
  assert(attached, "inlay hint provider not ready")

  vim.lsp.inlay_hint.enable(true, { bufnr = 0 })
  local rendered_ready = vim.wait(5000, function()
    return #vim.lsp.inlay_hint.get({ bufnr = 0 }) >= 4
  end, 50)
  assert(rendered_ready, "neovim did not materialize rendered inlay hints")

  local rendered_joined, rendered_positions = rendered_hints_by_label()
  assert(rendered_joined:match("src: Welcome"), "missing rendered plain source preview")
  assert(rendered_joined:match("src: %.label = Launch"), "missing rendered attribute source preview")
  assert(rendered_joined:match("src %[%s*platform=%*, tone=%*%s*%]: Press Ctrl %+ C to copy the download link now%."), "missing rendered selector source preview")
  assert(rendered_joined:match("src %[%s*platform=%*, tone=%*%s*%]: %.tooltip = Install the latest desktop build now%."), "missing rendered attribute selector source preview")
  assert(rendered_positions["src: Welcome"] and rendered_positions["src: Welcome"].character == 26, "rendered plain preview placed at wrong column")
  assert(rendered_positions["src: .label = Launch"] and rendered_positions["src: .label = Launch"].character == 19, "rendered attribute preview placed at wrong column")
  assert(
    rendered_positions["src [platform=*, tone=*]: Press Ctrl + C to copy the download link now."]
      and rendered_positions["src [platform=*, tone=*]: Press Ctrl + C to copy the download link now."].character == 14,
    "rendered selector preview placed at wrong column"
  )
  assert(
    rendered_positions["src [platform=*, tone=*]: .tooltip = Install the latest desktop build now."]
      and rendered_positions["src [platform=*, tone=*]: .tooltip = Install the latest desktop build now."].character == 14,
    "rendered attribute selector preview placed at wrong column"
  )

  local responses = vim.lsp.buf_request_sync(0, "textDocument/inlayHint", range_params(), 5000)
  local hints = assert(responses[client_id] and responses[client_id].result, "missing inlay hints")
  local labels = {}
  local positions = {}
  for _, hint in ipairs(hints) do
    if type(hint.label) == "string" then
      table.insert(labels, hint.label)
      positions[hint.label] = hint.position
    end
  end
  local joined = table.concat(labels, "\n")
  assert(joined:match("src: Welcome"), "missing plain source preview")
  assert(joined:match("src: %.label = Launch"), "missing attribute source preview")
  assert(joined:match("src %[%s*platform=%*, tone=%*%s*%]: Press Ctrl %+ C to copy the download link now%."), "missing selector source preview")
  assert(joined:match("src %[%s*platform=%*, tone=%*%s*%]: %.tooltip = Install the latest desktop build now%."), "missing attribute selector source preview")
  assert(positions["src: Welcome"] and positions["src: Welcome"].character == 26, "plain preview placed at wrong column")
  assert(positions["src: .label = Launch"] and positions["src: .label = Launch"].character == 19, "attribute preview placed at wrong column")
  assert(
    positions["src [platform=*, tone=*]: Press Ctrl + C to copy the download link now."]
      and positions["src [platform=*, tone=*]: Press Ctrl + C to copy the download link now."].character == 14,
    "selector preview placed at wrong column"
  )
  assert(
    positions["src [platform=*, tone=*]: .tooltip = Install the latest desktop build now."]
      and positions["src [platform=*, tone=*]: .tooltip = Install the latest desktop build now."].character == 14,
    "attribute selector preview placed at wrong column"
  )

  local filtered = vim.lsp.buf_request_sync(0, "textDocument/inlayHint", range_params(8, 14, 8, 15), 5000)
  local filtered_hints = assert(filtered[client_id] and filtered[client_id].result, "missing filtered inlay hints")
  assert(#filtered_hints == 1, "expected one filtered inlay hint")
  assert(
    filtered_hints[1].label == "src [platform=*, tone=*]: Press Ctrl + C to copy the download link now.",
    "filtered range returned the wrong inlay hint"
  )

  vim.cmd.edit(workspace .. "/locales/en/app.ftl")
  vim.lsp.buf_attach_client(0, client_id)
  vim.lsp.inlay_hint.enable(true, { bufnr = 0 })
  local origin_rendered_ready = vim.wait(5000, function()
    return #vim.lsp.inlay_hint.get({ bufnr = 0 }) >= 4
  end, 50)
  assert(origin_rendered_ready, "neovim did not materialize origin-file inlay hints")
  local origin_joined, origin_positions = rendered_hints_by_label()
  assert(origin_joined:match("src: Welcome"), "missing origin-file source preview")
  assert(origin_positions["src: Welcome"] and origin_positions["src: Welcome"].character == 23, "origin-file preview placed at wrong column")

  write_result(result_path, {
    ok = true,
    feature = "inlay_hint_source_preview",
  })
  vim.cmd.qa()
end

return M
