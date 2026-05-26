// mty-go is a small CLI front-end to the Go third-implementation of the
// Mighty front-end. It supports `lex` and `parse` subcommands that mirror
// the Rust toolchain's `mty dump --tokens` / `--cst` flavour.
package main

import (
	"fmt"
	"os"
	"strings"

	"github.com/hassard0/mighty-impl-go/mty"
)

func main() {
	if len(os.Args) < 3 {
		usage()
		os.Exit(2)
	}
	cmd := os.Args[1]
	file := os.Args[2]
	src, err := os.ReadFile(file)
	if err != nil {
		fmt.Fprintf(os.Stderr, "mty-go: cannot read %s: %v\n", file, err)
		os.Exit(1)
	}
	switch cmd {
	case "lex":
		runLex(string(src), file)
	case "parse":
		runParse(string(src), file)
	default:
		usage()
		os.Exit(2)
	}
}

func usage() {
	fmt.Fprintln(os.Stderr, "usage: mty-go lex <file>")
	fmt.Fprintln(os.Stderr, "       mty-go parse <file>")
}

func runLex(src, name string) {
	toks, diags := mty.Lex(src)
	for _, t := range toks {
		if t.Kind == mty.TK_EOF {
			break
		}
		fmt.Printf("%s  %s\n", spanStr(src, t.Start, t.End), t.Kind.Name())
	}
	for _, d := range diags {
		line, col := mty.LineCol(src, d.Span.Start)
		fmt.Fprintf(os.Stderr, "%s:%d:%d: %s: %s [%s]\n", name, line, col, d.Severity, d.Message, d.Code)
	}
	if len(diags) > 0 {
		os.Exit(1)
	}
}

func runParse(src, name string) {
	file, diags := mty.Parse(src)
	printNode(file, 0)
	for _, d := range diags {
		line, col := mty.LineCol(src, d.Span.Start)
		fmt.Fprintf(os.Stderr, "%s:%d:%d: %s: %s [%s]\n", name, line, col, d.Severity, d.Message, d.Code)
	}
	if len(diags) > 0 {
		os.Exit(1)
	}
}

func spanStr(src string, start, end int) string {
	line, col := mty.LineCol(src, start)
	return fmt.Sprintf("%d:%d..%d", line, col, end-start)
}

func printNode(n mty.Node, depth int) {
	if n == nil {
		return
	}
	fmt.Printf("%s%s\n", strings.Repeat("  ", depth), n.NodeKind())
	for _, c := range n.Children() {
		printNode(c, depth+1)
	}
}
