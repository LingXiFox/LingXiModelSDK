package main

import "html/template"

type templateSet = template.Template

func mustParsePage(pagePath string) *template.Template {
	return template.Must(
		template.New("page").
			Funcs(templateFuncs).
			ParseFS(assets, "templates/partials/*.html", pagePath),
	)
}
