#!/usr/bin/env python3
"""Exporta la memoria del TFM (Markdown) a DOCX y PDF.

Dependencias: scripts/requirements-tfm.txt. No consulta servicios ni modifica el
código del producto. El anexo A se incorpora desde docs/ESTUDIO_RED_OBRERA.md.

La fuente Markdown empieza con un bloque de metadatos delimitado por `---`
(titulo, autor, director, titulacion, curso, fecha). Con él se generan la portada
y la página de identificación según la plantilla de la UEM; después se insertan
el índice de contenidos, el índice de figuras y el índice de tablas.

Figuras: `![Descripción](figuras/archivo.png)` en un párrafo propio.
Tablas: un comentario `<!-- tabla: Descripción -->` en la línea anterior fija
su pie; si falta, se usa el título de la sección.
"""
import argparse
from hashlib import sha256
from html import escape
import json
from pathlib import Path
import re

from docx import Document
from docx.enum.text import WD_ALIGN_PARAGRAPH, WD_BREAK
from docx.oxml import OxmlElement
from docx.oxml.ns import qn
from docx.shared import Cm, Pt
from markdown_it import MarkdownIt
from PIL import Image as PILImage
from reportlab.lib import colors
from reportlab.lib.enums import TA_CENTER, TA_JUSTIFY
from reportlab.lib.pagesizes import A4
from reportlab.lib.styles import ParagraphStyle, getSampleStyleSheet
from reportlab.pdfbase import pdfmetrics
from reportlab.pdfbase.ttfonts import TTFont
from reportlab.platypus import (BaseDocTemplate, Frame, Image, KeepTogether, PageBreak,
                                PageTemplate, Paragraph, Spacer, Table, TableStyle)
from reportlab.platypus.tableofcontents import TableOfContents

ROOT = Path(__file__).resolve().parents[1]
MARGIN = 70.87  # 2,5 cm
TEXT_WIDTH = A4[0] - 2 * MARGIN


def parse_front_matter(text):
    match = re.match(r'^---\n(.*?)\n---\n', text, re.S)
    if not match:
        raise SystemExit('La memoria debe empezar con un bloque de metadatos ---')
    meta = {}
    for line in match.group(1).splitlines():
        key, _, value = line.partition(':')
        meta[key.strip()] = value.strip()
    return meta, text[match.end():]


class EntryList(TableOfContents):
    """Índice que solo escucha una clase de entradas (figuras o tablas)."""
    kind = 'TOCEntry'

    def notify(self, kind, stuff):
        if kind == self.kind:
            self.addEntry(*stuff)


class FigureList(EntryList):
    kind = 'FigEntry'


class TableList(EntryList):
    kind = 'TabEntry'


class MemoriaTemplate(BaseDocTemplate):
    def __init__(self, path, header, **kw):
        super().__init__(path, **kw)
        self.header = header
        frame = Frame(MARGIN, MARGIN, TEXT_WIDTH, A4[1] - 2 * MARGIN, id='body',
                      leftPadding=0, rightPadding=0, topPadding=0, bottomPadding=0)
        self.addPageTemplates([PageTemplate(id='cover', frames=[frame], onPage=self.blank),
                               PageTemplate(id='body', frames=[frame], onPage=self.decorate)])

    def blank(self, canvas, doc):
        pass

    def decorate(self, canvas, doc):
        canvas.saveState()
        canvas.setFont('Body', 8)
        canvas.setFillColor(colors.HexColor('#596574'))
        canvas.drawString(MARGIN, A4[1] - 42, self.header)
        canvas.drawCentredString(A4[0] / 2, 35, str(doc.page))
        canvas.restoreState()

    def afterFlowable(self, flowable):
        kind = getattr(flowable, 'entry_kind', None)
        if kind:
            self.notify(kind, (flowable.entry_level, flowable.entry_text, self.page))
            if kind == 'TOCEntry':
                key = 'h%d' % id(flowable)
                self.canv.bookmarkPage(key)
                self.canv.addOutlineEntry(flowable.entry_text, key, level=flowable.entry_level, closed=False)


def register_fonts():
    font_dir = Path('/usr/share/fonts/liberation')
    if not (font_dir / 'LiberationSans-Regular.ttf').is_file():
        raise SystemExit('Faltan fuentes Liberation Sans/Mono para generar el PDF')
    for label, file in [('Body', 'LiberationSans-Regular.ttf'), ('Body-Bold', 'LiberationSans-Bold.ttf'),
                        ('Body-Italic', 'LiberationSans-Italic.ttf'),
                        ('Body-BoldItalic', 'LiberationSans-BoldItalic.ttf')]:
        pdfmetrics.registerFont(TTFont(label, str(font_dir / file)))
    # El código y las ecuaciones del anexo usan símbolos (∈, ‖, ∇…) que Liberation
    # Mono no tiene; DejaVu Sans Mono, distribuida con matplotlib, sí los cubre.
    code_font = font_dir / 'LiberationMono-Regular.ttf'
    try:
        import matplotlib
        candidate = Path(matplotlib.__file__).parent / 'mpl-data/fonts/ttf/DejaVuSansMono.ttf'
        if candidate.is_file():
            code_font = candidate
    except ImportError:
        pass
    pdfmetrics.registerFont(TTFont('Code', str(code_font)))
    pdfmetrics.registerFontFamily('Body', normal='Body', bold='Body-Bold', italic='Body-Italic',
                                  boldItalic='Body-BoldItalic')
    return code_font.name.startswith('DejaVu')


def build_styles():
    styles = getSampleStyleSheet()
    styles.add(ParagraphStyle('Text', fontName='Body', fontSize=11, leading=16.5, spaceAfter=9, alignment=TA_JUSTIFY))
    styles.add(ParagraphStyle('BulletText', parent=styles['Text'], leftIndent=14, bulletIndent=2))
    styles.add(ParagraphStyle('Cell', fontName='Body', fontSize=9, leading=12, spaceAfter=2))
    styles.add(ParagraphStyle('Caption', fontName='Body-Italic', fontSize=9, leading=12, spaceAfter=14,
                              spaceBefore=4, alignment=TA_CENTER))
    styles.add(ParagraphStyle('CodeBlock', fontName='Code', fontSize=8, leading=10, spaceAfter=10,
                              backColor=colors.HexColor('#f1f3f5'), leftIndent=4, borderPadding=4))
    styles.add(ParagraphStyle('CoverCenter', fontName='Body-Bold', fontSize=13, leading=18, alignment=TA_CENTER,
                              spaceAfter=6))
    styles.add(ParagraphStyle('CoverTitle', fontName='Body-Bold', fontSize=18, leading=24, alignment=TA_CENTER,
                              spaceBefore=30, spaceAfter=30))
    styles.add(ParagraphStyle('CoverText', fontName='Body', fontSize=12, leading=18, alignment=TA_CENTER, spaceAfter=4))
    styles.add(ParagraphStyle('IdLine', fontName='Body', fontSize=12, leading=20, spaceAfter=10))
    styles.add(ParagraphStyle('IndexTitle', fontName='Body-Bold', fontSize=17, leading=22, spaceAfter=14))
    for n in range(1, 4):
        styles[f'Heading{n}'].fontName = 'Body-Bold'
        styles[f'Heading{n}'].fontSize = [17, 14, 12][n - 1]
        styles[f'Heading{n}'].leading = [22, 19, 16][n - 1]
        styles[f'Heading{n}'].spaceBefore = [15, 12, 10][n - 1]
        styles[f'Heading{n}'].spaceAfter = 12
        styles[f'Heading{n}'].keepWithNext = 1
    return styles


def toc_styles():
    base = dict(fontName='Body', fontSize=10.5, leading=15)
    return [ParagraphStyle('TOC1', fontName='Body-Bold', fontSize=11, leading=16, spaceBefore=4),
            ParagraphStyle('TOC2', leftIndent=16, **base),
            ParagraphStyle('TOC3', leftIndent=32, **base)]


def add_docx_field(paragraph, instr):
    run = paragraph.add_run()
    for tag, text in (('begin', None), ('instr', instr), ('separate', None), ('end', None)):
        if tag == 'instr':
            el = OxmlElement('w:instrText')
            el.set(qn('xml:space'), 'preserve')
            el.text = text
        else:
            el = OxmlElement('w:fldChar')
            el.set(qn('w:fldCharType'), tag)
        run._r.append(el)


def docx_caption(document, label, text):
    paragraph = document.add_paragraph(style='Caption')
    paragraph.alignment = WD_ALIGN_PARAGRAPH.CENTER
    paragraph.add_run(f'{label} ')
    add_docx_field(paragraph, f'SEQ {label} \\* ARABIC')
    paragraph.add_run(f'. {text}')


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--source', type=Path, default=ROOT / 'memoria/borrador-memoria-tfm.md')
    parser.add_argument('--docx', type=Path, default=ROOT / 'memoria/Memoria TFM - Quirón.docx')
    parser.add_argument('--pdf', type=Path, default=ROOT / 'memoria/Memoria TFM - Quirón.pdf')
    parser.add_argument('--report', type=Path, default=ROOT / 'docs/evidencias/2026-09-13/exportacion.json')
    args = parser.parse_args()
    raw = args.source.read_text()
    meta, body = parse_front_matter(raw)
    annex_path = ROOT / 'docs/ESTUDIO_RED_OBRERA.md'
    annex = annex_path.read_text()
    # Dentro de la memoria, el anexo cuelga de un único título de nivel 1.
    annex = re.sub(r'^(#{1,2}) ', r'#\1 ', annex, flags=re.M)
    dejavu = register_fonts()
    def clean(text):
        text = re.sub(r'[✅📐⚠🔬❌️]\ufe0f?\s?', '', text)
        # Símbolos sin glifo ni siquiera en DejaVu Sans Mono.
        text = text.replace('≪', '<<').replace('⟺', '<=>')
        if not dejavu:
            for src, dst in (('∈', 'in'), ('‖', '||'), ('⁺', '+'), ('∇', 'grad'), ('∼', '~')):
                text = text.replace(src, dst)
        return text
    combined = body + '\n\n# Anexo A. Estudio de alternativas para la red obrera\n\n' + annex
    tokens = MarkdownIt('commonmark').enable('table').parse(clean(combined))

    styles = build_styles()
    header = f"{meta['titulo_corto']} · {meta['autor']}"

    # ------------------------------------------------------------------ DOCX
    document = Document()
    section = document.sections[0]
    section.page_width, section.page_height = Cm(21), Cm(29.7)
    section.top_margin = section.bottom_margin = Cm(2.5)
    section.left_margin = section.right_margin = Cm(2.5)
    normal = document.styles['Normal']
    normal.font.name = 'Calibri'
    normal.font.size = Pt(11)
    normal.paragraph_format.line_spacing = 1.5
    normal.paragraph_format.space_after = Pt(8)
    for n in range(1, 4):
        document.styles[f'Heading {n}'].font.name = 'Calibri'
        document.styles[f'Heading {n}'].font.size = Pt([17, 14, 12][n - 1])
    section.header.paragraphs[0].text = header
    section.header.paragraphs[0].style = 'Caption'
    footer = section.footer.paragraphs[0]
    footer.alignment = WD_ALIGN_PARAGRAPH.CENTER
    add_docx_field(footer, 'PAGE')
    document.core_properties.title = meta['titulo']
    document.core_properties.author = meta['autor']
    document.core_properties.subject = 'Trabajo Fin de Máster'
    update = OxmlElement('w:updateFields')
    update.set(qn('w:val'), 'true')
    document.settings.element.append(update)

    def docx_center(text, bold=False, size=12, space=6):
        paragraph = document.add_paragraph()
        paragraph.alignment = WD_ALIGN_PARAGRAPH.CENTER
        paragraph.paragraph_format.space_after = Pt(space)
        run = paragraph.add_run(text)
        run.bold = bold
        run.font.size = Pt(size)

    docx_center('UNIVERSIDAD EUROPEA DE MADRID', True, 14, 30)
    docx_center('ESCUELA DE ARQUITECTURA, INGENIERÍA Y DISEÑO', True, 12, 18)
    docx_center(meta['titulacion'].upper(), True, 12, 40)
    docx_center('TRABAJO FIN DE MÁSTER', True, 13, 40)
    docx_center(meta['titulo'], True, 16, 50)
    docx_center(meta['autor'], False, 13, 24)
    docx_center('Dirigido por', False, 12, 4)
    docx_center(meta['director'], False, 13, 40)
    docx_center(f"CURSO {meta['curso']}", True, 12)
    document.add_page_break()
    for label, value in (('TÍTULO', meta['titulo']), ('AUTOR', meta['autor']), ('TITULACIÓN', meta['titulacion']),
                         ('DIRECTOR/ES DEL PROYECTO', meta['director']), ('FECHA', meta['fecha'])):
        paragraph = document.add_paragraph()
        paragraph.paragraph_format.space_after = Pt(18)
        paragraph.add_run(f'{label}: ').bold = True
        paragraph.add_run(value)
    for title, instr in (('Índice', 'TOC \\o "1-3" \\h \\z \\u'),
                         ('Índice de figuras', 'TOC \\h \\z \\c "Figura"'),
                         ('Índice de tablas', 'TOC \\h \\z \\c "Tabla"')):
        document.add_page_break()
        document.add_heading(title, level=1)
        add_docx_field(document.add_paragraph(), instr)

    # ------------------------------------------------------------------- PDF
    story = []

    def cover_pdf():
        out = [Spacer(1, 40), Paragraph('UNIVERSIDAD EUROPEA DE MADRID', styles['CoverCenter']), Spacer(1, 24),
               Paragraph('ESCUELA DE ARQUITECTURA, INGENIERÍA Y DISEÑO', styles['CoverCenter']), Spacer(1, 10),
               Paragraph(escape(meta['titulacion'].upper()), styles['CoverCenter']), Spacer(1, 50),
               Paragraph('TRABAJO FIN DE MÁSTER', styles['CoverCenter']), Spacer(1, 40),
               Paragraph(escape(meta['titulo']), styles['CoverTitle']), Spacer(1, 40),
               Paragraph(escape(meta['autor']), styles['CoverText']), Spacer(1, 24),
               Paragraph('Dirigido por', styles['CoverText']), Paragraph(escape(meta['director']), styles['CoverText']),
               Spacer(1, 50), Paragraph(f"CURSO {escape(meta['curso'])}", styles['CoverCenter']), PageBreak()]
        out.append(Spacer(1, 60))
        for label, value in (('TÍTULO', meta['titulo']), ('AUTOR', meta['autor']), ('TITULACIÓN', meta['titulacion']),
                             ('DIRECTOR/ES DEL PROYECTO', meta['director']), ('FECHA', meta['fecha'])):
            out.append(Paragraph(f'<b>{label}:</b> {escape(value)}', styles['IdLine']))
        return out

    story.extend(cover_pdf())
    from reportlab.platypus import NextPageTemplate
    story.append(NextPageTemplate('body'))
    story.append(PageBreak())
    toc = TableOfContents()
    toc.levelStyles = toc_styles()
    toc.dotsMinLevel = 0
    story.extend([Paragraph('Índice', styles['IndexTitle']), toc, PageBreak()])
    figures = FigureList()
    figures.levelStyles = [ParagraphStyle('FIG', fontName='Body', fontSize=10.5, leading=15)]
    figures.dotsMinLevel = 0
    story.extend([Paragraph('Índice de figuras', styles['IndexTitle']), figures, Spacer(1, 30)])
    tables = TableList()
    tables.levelStyles = [ParagraphStyle('TAB', fontName='Body', fontSize=10.5, leading=15)]
    tables.dotsMinLevel = 0
    story.extend([Paragraph('Índice de tablas', styles['IndexTitle']), tables])

    def html_inline(children):
        out = []
        for child in children or []:
            if child.type in ('text', 'html_inline'):
                out.append(escape(child.content))
            elif child.type == 'code_inline':
                out.append('<font name="Code">' + escape(child.content) + '</font>')
            elif child.type in ('softbreak', 'hardbreak'):
                out.append(' ')
            elif child.type == 'strong_open':
                out.append('<b>')
            elif child.type == 'strong_close':
                out.append('</b>')
            elif child.type == 'em_open':
                out.append('<i>')
            elif child.type == 'em_close':
                out.append('</i>')
            elif child.type == 'link_open':
                href = child.attrGet('href') or ''
                out.append('<link href="' + escape(href, quote=True) + '">' if href.startswith(('http://', 'https://')) else '<font>')
            elif child.type == 'link_close':
                opened = next((x for x in reversed(out) if x.startswith(('<link', '<font>'))), '')
                out.append('</link>' if opened.startswith('<link') else '</font>')
        return ''.join(out)

    def plain(children):
        return re.sub(r'<[^>]+>', '', html_inline(children))

    def doc_inline(paragraph, children, prefix=''):
        if prefix:
            paragraph.add_run(prefix)
        bold = italic = False
        for child in children or []:
            if child.type == 'strong_open':
                bold = True
            elif child.type == 'strong_close':
                bold = False
            elif child.type == 'em_open':
                italic = True
            elif child.type == 'em_close':
                italic = False
            elif child.type in ('text', 'code_inline', 'html_inline', 'softbreak', 'hardbreak'):
                run = paragraph.add_run(' ' if child.type in ('softbreak', 'hardbreak') else child.content)
                run.bold, run.italic = bold, italic
                if child.type == 'code_inline':
                    run.font.name = 'Consolas'
                    run.font.size = Pt(9)

    def figure_block(alt, src):
        path = (args.source.parent / src).resolve()
        with PILImage.open(path) as im:
            w, h = im.size
        width = min(TEXT_WIDTH, 15.5 * 28.35)
        height = width * h / w
        caption = Paragraph(escape(f'Figura {counters["fig"]}. {alt}'), styles['Caption'])
        caption.entry_kind, caption.entry_level, caption.entry_text = 'FigEntry', 0, f'Figura {counters["fig"]}. {alt}'
        story.append(KeepTogether([Image(str(path), width=width, height=height), caption]))
        document.add_picture(str(path), width=Cm(15.5))
        document.paragraphs[-1].alignment = WD_ALIGN_PARAGRAPH.CENTER
        docx_caption(document, 'Figura', alt)

    counters = {'fig': 1, 'tab': 1}
    i = 0
    list_depth = 0
    prefix = ''
    heading = ''
    pending_table_caption = None
    while i < len(tokens):
        token = tokens[i]
        if token.type == 'heading_open':
            inline = tokens[i + 1]
            level = min(int(token.tag[1]), 3)
            heading = plain(inline.children)
            if level == 1:
                document.add_page_break()
                story.append(PageBreak())
            doc_inline(document.add_heading(level=level), inline.children)
            paragraph = Paragraph(html_inline(inline.children), styles[f'Heading{level}'])
            paragraph.entry_kind, paragraph.entry_level, paragraph.entry_text = 'TOCEntry', level - 1, heading
            story.append(paragraph)
            i += 3
            continue
        if token.type == 'html_block':
            match = re.match(r'<!--\s*tabla:\s*(.*?)\s*-->', token.content.strip(), re.S)
            if match:
                pending_table_caption = match.group(1)
            i += 1
            continue
        if token.type == 'table_open':
            rows = []
            current = []
            j = i + 1
            while tokens[j].type != 'table_close':
                if tokens[j].type == 'tr_open':
                    current = []
                elif tokens[j].type == 'inline':
                    current.append(tokens[j])
                elif tokens[j].type == 'tr_close':
                    rows.append(current)
                j += 1
            caption_text = pending_table_caption or heading
            pending_table_caption = None
            table = document.add_table(rows=len(rows), cols=len(rows[0]))
            table.style = 'Table Grid'
            for row, cells in zip(table.rows, rows):
                for cell, value in zip(row.cells, cells):
                    doc_inline(cell.paragraphs[0], value.children)
                    for run in cell.paragraphs[0].runs:
                        run.font.size = Pt(9)
            for cell in table.rows[0].cells:
                for run in cell.paragraphs[0].runs:
                    run.bold = True
            docx_caption(document, 'Tabla', caption_text)
            cells = [[Paragraph(html_inline(t.children), styles['Cell']) for t in row] for row in rows]
            ncols = len(rows[0])
            if ncols == 2:
                widths = [TEXT_WIDTH * 0.32, TEXT_WIDTH * 0.68]
            elif ncols == 3:
                widths = [TEXT_WIDTH * 0.18, TEXT_WIDTH * 0.41, TEXT_WIDTH * 0.41]
            else:
                widths = [TEXT_WIDTH / ncols] * ncols
            pdf_table = Table(cells, colWidths=widths, repeatRows=1, hAlign='LEFT')
            pdf_table.setStyle(TableStyle([('BACKGROUND', (0, 0), (-1, 0), colors.HexColor('#e8edf4')),
                                           ('GRID', (0, 0), (-1, -1), .4, colors.HexColor('#b8c0ca')),
                                           ('VALIGN', (0, 0), (-1, -1), 'TOP'),
                                           ('LEFTPADDING', (0, 0), (-1, -1), 6), ('RIGHTPADDING', (0, 0), (-1, -1), 6),
                                           ('TOPPADDING', (0, 0), (-1, -1), 5), ('BOTTOMPADDING', (0, 0), (-1, -1), 5)]))
            caption = Paragraph(escape(f'Tabla {counters["tab"]}. {caption_text}'), styles['Caption'])
            caption.entry_kind, caption.entry_level, caption.entry_text = 'TabEntry', 0, f'Tabla {counters["tab"]}. {caption_text}'
            story.extend([pdf_table, caption])
            counters['tab'] += 1
            i = j + 1
            continue
        if token.type in ('bullet_list_open', 'ordered_list_open'):
            list_depth += 1
        elif token.type in ('bullet_list_close', 'ordered_list_close'):
            list_depth -= 1
        elif token.type == 'list_item_open':
            prefix = '• '
        elif token.type == 'paragraph_open':
            inline = tokens[i + 1]
            if inline.type == 'inline':
                children = inline.children or []
                if len(children) == 1 and children[0].type == 'image':
                    image = children[0]
                    figure_block(plain(image.children) or image.attrGet('alt') or '', image.attrGet('src'))
                    counters['fig'] += 1
                    i += 3
                    continue
                paragraph = document.add_paragraph()
                if list_depth:
                    paragraph.paragraph_format.left_indent = Cm(.5 * list_depth)
                doc_inline(paragraph, children, prefix)
                if prefix:
                    story.append(Paragraph(html_inline(children), styles['BulletText'], bulletText='•'))
                else:
                    story.append(Paragraph(html_inline(children), styles['Text']))
                prefix = ''
                i += 3
                continue
        elif token.type == 'fence':
            paragraph = document.add_paragraph()
            run = paragraph.add_run(token.content.rstrip())
            run.font.name = 'Consolas'
            run.font.size = Pt(9)
            code = escape(token.content.rstrip()).replace('\n', '<br/>').replace('  ', '&#160;&#160;')
            story.append(Paragraph(code, styles['CodeBlock']))
        elif token.type == 'blockquote_open':
            pass
        elif token.type == 'hr':
            story.append(Spacer(1, 12))
        i += 1

    args.docx.parent.mkdir(parents=True, exist_ok=True)
    document.save(args.docx)
    pdf = MemoriaTemplate(str(args.pdf), header, pagesize=A4, leftMargin=MARGIN, rightMargin=MARGIN,
                          topMargin=MARGIN, bottomMargin=MARGIN, title=meta['titulo'], author=meta['autor'])
    pdf.multiBuild(story)

    report = {'source_sha256': sha256(args.source.read_bytes()).hexdigest(),
              'annex_sha256': sha256(annex_path.read_bytes()).hexdigest(),
              'outputs': {p.name: sha256(p.read_bytes()).hexdigest() for p in (args.docx, args.pdf)},
              'pending_fields': raw.count('[COMPLETAR'), 'figures': counters['fig'] - 1,
              'tables': counters['tab'] - 1}
    args.report.parent.mkdir(parents=True, exist_ok=True)
    args.report.write_text(json.dumps(report, ensure_ascii=False, indent=2) + '\n')
    print(json.dumps(report, ensure_ascii=False, indent=2))


if __name__ == '__main__':
    main()
