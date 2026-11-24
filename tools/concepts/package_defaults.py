#!/usr/bin/env python3
"""Package independently qualified public defaults; never select or retune a cutoff.

Input is the explicit public-only scratch from the documented frozen protocol.
Original catalog, calibration report and quarantine remain byte-for-byte intact.
"""
import argparse
import copy
from pathlib import Path

import calibrate as tool


def package(scratch):
    baseline_path = tool.ROOT / 'gallery-backend/assets/concepts.json'
    baseline = tool.read_json(baseline_path)
    catalog = tool.read_json(scratch / 'policy-frozen-catalog.json')
    independent = tool.read_json(scratch / 'heldout/report.json')
    references = tool.read_json(scratch / 'references/frozen-heads.json')
    reference_report = tool.read_json(scratch / 'references/report.json')
    screen = tool.read_json(scratch / 'screen/calibration-report.json')
    fixtures = tool.read_json(scratch / 'qualified-fixtures.json')
    qualified_test = tool.read_json(scratch / 'qualified-report.json')
    qualified = {k for k, v in independent['categories'].items() if v['accepted']}
    if fixtures['vetoes'] or qualified != set(fixtures['qualified']):
        raise ValueError('qualified defaults failed independent fixture veto')
    if not baseline['model_id'] == references['model_id'] == catalog['model_id'] == independent['model']['model_id']:
        raise ValueError('public defaults model mismatch')
    original = {c['id']: c for c in baseline['concepts']}
    new = [c for c in catalog['concepts'] if c['id'] not in original and c['id'] in qualified]
    heads = {k: v for k, v in references['heads'].items() if k in qualified}
    for name in qualified:
        result = reference_report['concepts'][name] if name in heads else screen['concepts'][name]
        if result['status'] != 'calibrated' or not all(tool.enough(result[p], 5) and tool.passes(result[p], .9, .1, 5)
                                                         for p in ['selection', 'evaluation']):
            raise ValueError(f'{name} failed original selection/evaluation')
        measured = independent['categories'][name]
        if not tool.enough(measured, 5) or not tool.passes(measured, .9, .1, 5):
            raise ValueError(f'{name} failed frozen independent test')
    refinements = {c['id']: c['parents'] for c in catalog['concepts']
                   if c['id'] in original and c['parents'] != original[c['id']]['parents']}
    defaults = {'schema_version': 1, 'baseline_version': baseline['version'],
                'vocabulary_version': '2026-11-24.1', 'model_id': baseline['model_id'],
                'embedding_dim': 768, 'concepts': new, 'parent_refinements': refinements, 'heads': heads}
    path = tool.ROOT / 'gallery-backend/assets/concept-defaults.json'
    tool.write_json(path, defaults)
    assembled = copy.deepcopy(baseline)
    assembled['version'] = defaults['vocabulary_version']
    assembled['concepts'].extend(new)
    for c in assembled['concepts']:
        if c['id'] in refinements:
            c['parents'] = refinements[c['id']]
    tool.write_json(scratch / 'qualified-catalog.json', assembled)
    tool.write_json(scratch / 'qualified-heads.json', {'model_id': baseline['model_id'], 'heads': heads})
    report = {'schema_version': 1, 'model': independent['model'],
              'vocabulary_version': defaults['vocabulary_version'],
              'gates': {'observed_precision': .9, 'equal_prior_precision': .9, 'recall': .1,
                        'minimum_per_class': 5, 'minimum_predictions': 5},
              'baseline_catalog_sha256': tool.digest(baseline_path.read_bytes()),
              'defaults_sha256': tool.digest(path.read_bytes()),
              'sources_sha256': {str(p): tool.digest((scratch/p).read_bytes()) for p in [
                  Path('screen/calibration-report.json'), Path('references/frozen-heads.json'),
                  Path('references/report.json'), Path('heldout/report.json'), Path('qualified-fixtures.json')]},
              'zero_shot': {k: screen['concepts'][k] for k in sorted(qualified - heads.keys())},
              'references': {k: reference_report['concepts'][k] for k in sorted(heads)},
              'independent_test': independent,
              'qualified_test': qualified_test,
              'fixtures': fixtures,
              'limitations': 'Public human-label-conditional tests, not owner-photo precision. '
                             'Unknown annotations remain unknown; no claim of pretraining independence. '
                             'Eleven independent failures excluded without retuning. Owner feedback always wins.'}
    tool.write_json(tool.ROOT / 'tools/concepts/public-defaults-report.json', report)
    print(f'Packaged {len(new)} neutral Concepts, {len(heads)} reference heads; original {len(original)} thresholds untouched')


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--scratch', type=Path, required=True)
    package(parser.parse_args().scratch)
