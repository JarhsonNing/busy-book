// Preserve typed option values: null, empty string, numeric IDs and remote string IDs are distinct.
export function optionText(children) {
  if (Array.isArray(children)) return children.map((v) => optionText(v?.children ?? v)).join('');
  return typeof children === 'string' || typeof children === 'number' ? String(children) : '';
}
export function collectOptions(nodes, result = []) {
  for (const node of nodes || []) {
    if (node?.type === 'option') {
      const label = optionText(node.children).trim();
      result.push({
        value: Object.hasOwn(node.props || {}, 'value') ? node.props.value : label,
        label,
        disabled: node.props?.disabled === '' || node.props?.disabled === true,
      });
    } else if (Array.isArray(node?.children)) collectOptions(node.children, result);
  }
  return result;
}
export function filterOptions(options, query) {
  const terms = query.trim().toLocaleLowerCase().split(/\s+/).filter(Boolean);
  return options.filter((o) =>
    terms.every((term) => `${o.label} ${o.value ?? ''}`.toLocaleLowerCase().includes(term)),
  );
}
