// End-to-end journey of M7's background image or PDF (run by run.mjs after the main journeys).
//
// A two-page PDF is written next to the run (a hand-made, valid PDF: page 1 has a black square in
// its top-left corner, page 2 one in its bottom-right corner), imported from the library as two
// notes (« Depuis un PDF ou une image »), the first one is opened, turned a quarter turn to the
// right, and exported as PNG: the square of the PDF must now be in the top-right corner of the
// image, the rest of the page white. The native dialogs cannot be driven: the interface lets the
// journey script their answers (`window.__ardoiseE2eOpenPath`, `window.__ardoiseE2eSavePath`,
// see `lib/dialogs.ts`).
import { existsSync, readFileSync, writeFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { inflateSync } from 'node:zlib';

const locales = join(dirname(fileURLToPath(import.meta.url)), '..', 'ui', 'src', 'locales');
const catalogs = {
  fr: JSON.parse(readFileSync(join(locales, 'fr.json'), 'utf8')),
  en: JSON.parse(readFileSync(join(locales, 'en.json'), 'utf8')),
};
const text = (language, key) => key.split('.').reduce((node, part) => node?.[part], catalogs[language]);

/**
 * A valid two-page A4 portrait PDF (595 × 842 pt). Each page fills a 200 pt black square: page 1
 * at its top-left corner (from 40 pt of the edges), page 2 at its bottom-right corner.
 */
export function twoPagePdf() {
  const squares = ['40 602 200 200 re f', '355 40 200 200 re f'];
  const objects = [
    '<< /Type /Catalog /Pages 2 0 R >>',
    '<< /Type /Pages /Kids [3 0 R 5 0 R] /Count 2 >>',
  ];
  for (const [i, square] of squares.entries()) {
    const content = `0 0 0 rg\n${square}\n`;
    const page = 3 + i * 2;
    objects.push(`<< /Type /Page /Parent 2 0 R /MediaBox [0 0 595 842] /Contents ${page + 1} 0 R >>`);
    objects.push(`<< /Length ${content.length} >>\nstream\n${content}endstream`);
  }
  let pdf = '%PDF-1.4\n';
  const offsets = objects.map((body, i) => {
    const offset = pdf.length;
    pdf += `${i + 1} 0 obj\n${body}\nendobj\n`;
    return offset;
  });
  const xref = pdf.length;
  pdf += `xref\n0 ${objects.length + 1}\n0000000000 65535 f \n`;
  pdf += offsets.map((o) => `${String(o).padStart(10, '0')} 00000 n \n`).join('');
  pdf += `trailer\n<< /Size ${objects.length + 1} /Root 1 0 R >>\nstartxref\n${xref}\n%%EOF\n`;
  return Buffer.from(pdf, 'latin1');
}

/** The pixels of an 8-bit RGB or RGBA PNG (what Ardoise writes): `{ width, height, rgb(x, y) }`. */
export function readPng(bytes) {
  if (bytes.subarray(0, 8).toString('latin1') !== '\x89PNG\r\n\x1a\n') throw new Error('not a PNG');
  let at = 8;
  let width = 0;
  let height = 0;
  let channels = 0;
  const data = [];
  while (at < bytes.length) {
    const length = bytes.readUInt32BE(at);
    const type = bytes.subarray(at + 4, at + 8).toString('latin1');
    const body = bytes.subarray(at + 8, at + 8 + length);
    if (type === 'IHDR') {
      width = body.readUInt32BE(0);
      height = body.readUInt32BE(4);
      const [depth, color, , , interlace] = body.subarray(8, 13);
      if (depth !== 8 || interlace !== 0 || (color !== 2 && color !== 6)) throw new Error('unexpected PNG layout');
      channels = color === 2 ? 3 : 4;
    } else if (type === 'IDAT') {
      data.push(body);
    }
    at += 12 + length;
  }
  const raw = inflateSync(Buffer.concat(data));
  const stride = width * channels;
  const pixels = Buffer.alloc(stride * height);
  for (let y = 0; y < height; y++) {
    const filter = raw[y * (stride + 1)];
    for (let x = 0; x < stride; x++) {
      const value = raw[y * (stride + 1) + 1 + x];
      const left = x >= channels ? pixels[y * stride + x - channels] : 0;
      const up = y > 0 ? pixels[(y - 1) * stride + x] : 0;
      const corner = x >= channels && y > 0 ? pixels[(y - 1) * stride + x - channels] : 0;
      const paeth = () => {
        const p = left + up - corner;
        const [pa, pb, pc] = [Math.abs(p - left), Math.abs(p - up), Math.abs(p - corner)];
        return pa <= pb && pa <= pc ? left : pb <= pc ? up : corner;
      };
      const predicted = [0, left, up, (left + up) >> 1, paeth()][filter];
      pixels[y * stride + x] = (value + predicted) & 0xff;
    }
  }
  const rgb = (x, y) => [...pixels.subarray((y * width + x) * channels, (y * width + x) * channels + 3)];
  return { width, height, rgb };
}

export const backgroundJourneys = [
  {
    name: 'un PDF de deux pages devient deux notes ; une note tournée s’exporte en PNG avec sa page en fond',
    async run(app, check) {
      const l = (key) => text(app.language, key);
      const j = JSON.stringify;
      const pdf = join(app.dir, 'fiche-e2e.pdf');
      const png = join(app.dir, 'fiche-tournee.png');
      const DIALOG = '[role="dialog"]';
      const SECTION = '[data-section="background-image"]';
      writeFileSync(pdf, twoPagePdf());

      await check('la bibliothèque de démonstration s’affiche (7 notes)', async () => {
        await app.waitFor(`document.querySelectorAll('.note-card').length === 7`, 'the 7 demo notes');
      });

      await check(`« ${l('background.library.open')} » lit le PDF : deux pages`, async () => {
        await app.evaluate(`window.__ardoiseE2eOpenPath = ${j(pdf)}`);
        await app.click('button[data-action="new-from-file"]');
        await app.waitFor(
          `document.querySelector(${j(`${DIALOG} [data-slot="new-from-file-name"]`)})?.textContent === 'fiche-e2e.pdf'`,
          'the chosen PDF',
        );
        const all = l('background.library.allPages').replace('{count}', '2');
        await app.waitFor(`document.querySelector(${j(DIALOG)}).textContent.includes(${j(all)})`, 'its two pages');
      });

      await check('créer les deux notes : 9 notes dans la grille', async () => {
        await app.click(`${DIALOG} .actions button.primary`);
        await app.waitFor(`document.querySelector(${j(DIALOG)}) === null`, 'the dialog to close', 30_000);
        await app.waitFor(`document.querySelectorAll('.note-card').length === 9`, 'the two new notes', 15_000);
      });

      await check('ouvrir « fiche-e2e - page 1 » : l’éditeur montre son image de fond', async () => {
        await app.click('.note-card .card-open', 'fiche-e2e - page 1');
        await app.waitFor(
          `document.querySelector(${j(`${SECTION} [data-slot="background-image-name"]`)})?.textContent.includes('fiche-e2e.pdf')`,
          'the background image section',
        );
      });

      await check(`tourner la page d’un quart de tour (« ${l('editor.page.rotateCwShort')} »)`, async () => {
        await app.click(`button[title=${j(l('editor.page.rotateCw'))}]`);
        await app.waitFor(`document.querySelector('.editor').dataset.dirty === 'yes'`, 'the unsaved mark');
      });

      await check('exporter en PNG (note non enregistrée, avec son fond)', async () => {
        await app.click('.toolbar button', l('editor.export.button'));
        await app.waitFor(`document.querySelector(${j(`${DIALOG} h2`)})?.textContent === ${j(l('export.title'))}`, 'the export dialog');
        await app.click(`${DIALOG} label.format-card`, l('export.formats.png'));
        await app.evaluate(`window.__ardoiseE2eSavePath = ${j(png)}`);
        await app.click(`${DIALOG} .actions button.primary`);
        await app.waitFor(`document.querySelector(${j(`${DIALOG} .export-result code`)})?.textContent === ${j(png)}`, 'the export result', 60_000);
        await app.click(`${DIALOG} .actions button`, l('common.close'));
      });

      await check('le PNG est en paysage et le carré noir du PDF est passé en haut à droite', async () => {
        if (!existsSync(png)) throw new Error(`${png} was not written`);
        const image = readPng(readFileSync(png));
        if (image.width <= image.height) throw new Error(`a portrait image after the turn: ${image.width} x ${image.height}`);
        const at = (fx, fy) => image.rgb(Math.floor(image.width * fx), Math.floor(image.height * fy));
        const dark = (c) => c.every((v) => v < 80);
        const white = (c) => c.every((v) => v > 230);
        // The square covered 40..240 pt of 595 from the left and 40..240 pt of 842 from the top;
        // turned clockwise, it covers 40..240 / 842 from the right and 40..240 / 595 from the top.
        const square = at(1 - 140 / 842, 140 / 595);
        if (!dark(square)) throw new Error(`no black square at the top right: ${square}`);
        for (const [fx, fy] of [[140 / 842, 140 / 595], [0.5, 0.5], [1 - 140 / 842, 1 - 140 / 595]]) {
          const c = at(fx, fy);
          if (!white(c)) throw new Error(`not the white page at (${fx.toFixed(2)}, ${fy.toFixed(2)}): ${c}`);
        }
      });
    },
  },
];
