#include "ast/regex_literal_splitting.hpp"
#include <cctype>
#include <limits>
#include <utility>
#include <iterator>
#include <cstdio> // for printf
#include <string>
#include <vector>

namespace {
std::pair<size_t, size_t> get_quant_n(const std::string& quant_str) {
    static const size_t INF = std::numeric_limits<size_t>::max();
    if (quant_str == "*") return {0, INF};
    if (quant_str == "+") return {1, INF};
    if (quant_str == "?") return {0, 1};
    if (quant_str.size() > 1 && quant_str.front() == '{' && quant_str.back() == '}') {
        std::string inner = quant_str.substr(1, quant_str.size() - 2);
        try {
            if (inner.find(',') == std::string::npos) {
                size_t n = std::stoul(inner);
                return {n, n};
            } else {
                size_t comma_pos = inner.find(',');
                std::string min_s = inner.substr(0, comma_pos);
                std::string max_s = inner.substr(comma_pos + 1);
                size_t min_n = 0;
                if (!min_s.empty()) {
                    min_n = std::stoul(min_s);
                }
                size_t max_n = INF;
                if (!max_s.empty()) {
                    max_n = std::stoul(max_s);
                }
                return {min_n, max_n};
            }
        } catch (...) {
            // Invalid, fall through
        }
    }
    return {0, 0}; // Invalid
}

void merge_adj_lits(std::vector<Segment>& path) {
    if (path.empty()) return;
    size_t write_idx = 0;
    for (size_t read_idx = 1; read_idx < path.size(); ++read_idx) {
        if (path[write_idx].is_lit == path[read_idx].is_lit) {
            path[write_idx].str += path[read_idx].str;
        } else {
            ++write_idx;
            if (write_idx != read_idx) {
                path[write_idx] = std::move(path[read_idx]);
            }
        }
    }
    path.resize(write_idx + 1);
}

// Forward declarations
std::vector<std::vector<Segment>> parseRE(const std::string& s, size_t& pos);
std::vector<std::vector<Segment>> parseAlt(const std::string& s, size_t& pos);
std::vector<std::vector<Segment>> parseConcat(const std::string& s, size_t& pos);
std::vector<std::vector<Segment>> parseTerm(const std::string& s, size_t& pos);
std::vector<std::vector<Segment>> parseAtom(const std::string& s, size_t& pos);

std::vector<std::vector<Segment>> parseAtom(const std::string& s, size_t& pos) {
    if (pos >= s.size()) {
        return {};
    }
    char c = s[pos];
    if (c == '\\') {
        ++pos;
        if (pos >= s.size()) {
            return {};
        }
        char esc = s[pos++];
        // Handle common escapes as non-literals
        if (esc == 'd') {
            Segment seg{"\\d", false};
            return {{seg}};
        } else if (esc == 'w') {
            Segment seg{"\\w", false};
            return {{seg}};
        } else if (esc == 's') {
            Segment seg{"\\s", false};
            return {{seg}};
        } else if (esc == 'D') {
            Segment seg{"\\D", false};
            return {{seg}};
        } else if (esc == 'W') {
            Segment seg{"\\W", false};
            return {{seg}};
        } else if (esc == 'S') {
            Segment seg{"\\S", false};
            return {{seg}};
        } else if (esc == 'b') {
            Segment seg{"\\b", false};
            return {{seg}};
        } else if (esc == 'B') {
            Segment seg{"\\B", false};
            return {{seg}};
        } else if (esc == 'A') {
            Segment seg{"\\A", false};
            return {{seg}};
        } else if (esc == 'Z') {
            Segment seg{"\\Z", false};
            return {{seg}};
        } else if (esc == 'z') {
            Segment seg{"\\z", false};
            return {{seg}};
        } else if (esc == 'R') {
            Segment seg{"\\R", false};
            return {{seg}};
        } else if (esc == 'p' || esc == 'P') {
            // Simple \p{...} or \P{...} - parse until }
            size_t start = pos - 2;  // Back to \p or \P
            while (pos < s.size() && s[pos] != '}') {
                ++pos;
            }
            if (pos < s.size()) {
                ++pos;
            }
            std::string prop_esc = s.substr(start, pos - start);
            Segment seg{std::move(prop_esc), false};
            return {{seg}};
        } else if (esc == 'Q') {
            // \Q...\E - treat everything until \E as literal
            size_t q_start = pos;
            bool found_e = false;
            while (pos + 1 < s.size()) {
                if (s[pos] == '\\' && s[pos + 1] == 'E') {
                    std::string quoted = s.substr(q_start, pos - q_start);
                    Segment seg{std::move(quoted), true};
                    pos += 2;
                    found_e = true;
                    return {{seg}};
                }
                ++pos;
            }
            // No \E found, treat from after \Q to end as literal
            std::string quoted = s.substr(q_start, s.size() - q_start);
            Segment seg{std::move(quoted), true};
            pos = s.size();
            return {{seg}};
        } else {
            // Other escapes as literal char
            Segment seg{std::string(1, esc), true};
            return {{seg}};
        }
    } else if (c == '.') {
        ++pos;
        Segment seg{".", false};
        return {{seg}};
    } else if (c == '[') {
        size_t start = pos;
        ++pos;
        while (pos < s.size() && s[pos] != ']') {
            if (s[pos] == '\\') ++pos;
            if (pos < s.size()) ++pos;
        }
        if (pos < s.size() && s[pos] == ']') ++pos;
        std::string class_str = s.substr(start, pos - start);
        Segment seg{std::move(class_str), false};
        return {{seg}};
    } else if (c == '(') {
        ++pos;
        // Basic group support; extend for (?= etc. in future
        std::vector<std::vector<Segment>> paths = parseRE(s, pos);
        if (pos < s.size() && s[pos] == ')') ++pos;
        return paths;
    } else if (c == '^' || c == '$') {
        ++pos;
        Segment seg{std::string(1, c), false};
        return {{seg}};
    } else {
        // Literal
        ++pos;
        Segment seg{std::string(1, c), true};
        return {{seg}};
    }
}

std::vector<std::vector<Segment>> parseTerm(const std::string& s, size_t& pos) {
    std::vector<std::vector<Segment>> paths = parseAtom(s, pos);
    if (paths.empty()) {
        return {};
    }
    // Quantifier parsing (unchanged)
    std::string quant_str;
    bool has_quant = false;
    if (pos < s.size()) {
        char qc = s[pos];
        if (qc == '*' || qc == '+' || qc == '?') {
            quant_str = std::string(1, qc);
            ++pos;
            has_quant = true;
        } else if (qc == '{') {
            size_t brace_start = pos;
            ++pos;
            // Skip digits for min (unchanged)
            while (pos < s.size() && std::isdigit(static_cast<unsigned char>(s[pos]))) ++pos;
            if (pos < s.size() && s[pos] == ',') ++pos;
            // Skip digits for max (unchanged)
            while (pos < s.size() && std::isdigit(static_cast<unsigned char>(s[pos]))) ++pos;
            if (pos < s.size() && s[pos] == '}') {
                ++pos;
                quant_str = s.substr(brace_start, pos - brace_start);
                has_quant = true;
            } else {
                pos = brace_start; // Invalid, ignore
            }
        }
    }
    // Handle lazy/possessive (unchanged)
    std::string full_quant = quant_str;
    bool is_lazy = false;
    bool is_possessive = false;
    if (has_quant && pos < s.size()) {
        if (s[pos] == '?') {
            full_quant += '?';
            is_lazy = true;
            ++pos;
        } else if (s[pos] == '+') {
            full_quant += '+';
            is_possessive = true;
            ++pos;
        }
    }
    if (has_quant) {
        auto [min_n, max_n] = get_quant_n(quant_str); // Use base quant for min/max
        if (quant_str == "?") { // Special case for ? (unchanged)
            std::vector<std::vector<Segment>> new_paths;
            new_paths.emplace_back(); // 0 times: empty
            for (const auto& p : paths) {
                std::vector<Segment> np;
                for (const auto& seg : p) {
                    np.emplace_back(seg.str, seg.is_lit);
                }
                merge_adj_lits(np);
                new_paths.push_back(std::move(np));
            }
            return new_paths;
        }
        if (max_n != std::numeric_limits<size_t>::max() && max_n == min_n && min_n > 0) {
            // Fixed repetition: expand (unchanged)
            size_t n = min_n;
            std::vector<std::vector<Segment>> repeated{{}};
            for (size_t k = 0; k < n; ++k) {
                std::vector<std::vector<Segment>> new_rep;
                for (const auto& pre : repeated) {
                    for (const auto& p : paths) {
                        std::vector<Segment> np;
                        for (const auto& seg : pre) np.emplace_back(seg.str, seg.is_lit);
                        for (const auto& seg : p) np.emplace_back(seg.str, seg.is_lit);
                        merge_adj_lits(np);
                        new_rep.push_back(std::move(np));
                    }
                }
                repeated = std::move(new_rep);
            }
            // If lazy/possessive, tag the whole repeated as non-lit? But for simplicity, keep expanded
            return repeated;
        }
        // *** NEW: Variable, lazy, or possessive: group quant with last segment of each path ***
        std::vector<std::vector<Segment>> new_paths;
        for (const auto& p : paths) {
            std::vector<Segment> new_p = p;  // Copy path
            if (!new_p.empty()) {
                // Append to last segment
                new_p.back().str += full_quant;
                // Downgrade to non-lit if it was lit (quant makes it variable)
                if (new_p.back().is_lit) {
                    new_p.back().is_lit = false;
                }
            } else {
                // Rare: empty atom + quant? Append as non-lit
                new_p.emplace_back(full_quant, false);
            }
            merge_adj_lits(new_p);  // Re-merge if needed (e.g., prior non-lits)
            new_paths.push_back(std::move(new_p));
        }
        if (min_n == 0) {
            new_paths.insert(new_paths.begin(), {});
        }
        return new_paths;
    }
    return paths;  // No quant: unchanged
}

std::vector<std::vector<Segment>> parseConcat(const std::string& s, size_t& pos) {
    std::vector<std::vector<std::vector<Segment>>> sub_groups;
    while (pos < s.size() && s[pos] != '|' && s[pos] != ')') {
        std::vector<std::vector<Segment>> sub = parseTerm(s, pos);
        if (!sub.empty()) {
            sub_groups.push_back(std::move(sub));
        } else {
            // If sub empty, treat next char as lit if possible, but for now break
            break;
        }
    }
    std::vector<std::vector<Segment>> current{{}}; // Start with one empty path
    for (const auto& group : sub_groups) {
        std::vector<std::vector<Segment>> new_current;
        for (const auto& prefix : current) {
            for (const auto& suffix_group : group) {
                std::vector<Segment> new_path;
                for (const auto& seg : prefix) {
                    new_path.emplace_back(seg.str, seg.is_lit);
                }
                for (const auto& seg : suffix_group) {
                    new_path.emplace_back(seg.str, seg.is_lit);
                }
                merge_adj_lits(new_path);
                new_current.push_back(std::move(new_path));
            }
        }
        current = std::move(new_current);
    }
    return current;
}

std::vector<std::vector<Segment>> parseAlt(const std::string& s, size_t& pos) {
    std::vector<std::vector<Segment>> paths = parseConcat(s, pos);
    while (pos < s.size() && s[pos] == '|') {
        ++pos;
        std::vector<std::vector<Segment>> sub = parseConcat(s, pos);
        paths.insert(paths.end(), std::make_move_iterator(sub.begin()), std::make_move_iterator(sub.end()));
    }
    return paths;
}

std::vector<std::vector<Segment>> parseRE(const std::string& s, size_t& pos) {
    return parseAlt(s, pos);
}
} // anonymous namespace

std::vector<std::vector<Segment>> regex_literal_splitting(const std::string& p_pattern) {
    std::string str = p_pattern;
    size_t pos = 0;
    std::vector<std::vector<Segment>> paths = parseRE(str, pos);
    // Handle tail if not fully consumed: append as non-literal to each path
    if (pos < str.size()) {
        std::string tail = str.substr(pos);
        if (paths.empty()) {
            // Fallback: whole as non-lit
            paths.emplace_back();
            paths.back().emplace_back(std::move(tail), false);
        } else {
            for (auto& path : paths) {
                path.emplace_back(std::move(tail), false);
            }
        }
    }
    // Note: No error checking; assumes valid regex where possible
    return paths;
}

void test_pattern(const std::string& pattern) {
    std::vector<std::vector<Segment>> paths = regex_literal_splitting(pattern);
    if (paths.empty()) {
        printf("Failed to extract paths\n");
        return;
    }
    printf("%s\n", pattern.c_str());
    for (const auto& path : paths) {
        printf("    ");
        for (const auto& seg : path) {
            printf("%s  ", seg.str.c_str());
        }
        printf("\n");
    }
}

extern "C" {
void regex_literal_splitting_test(void) {
    // Original tests
    test_pattern(R"((/\*.*\*/))");
    test_pattern(R"((ads/\*.*\*/))");
    test_pattern(R"(a.b.c)");
    test_pattern(R"((a|b).c)");
    test_pattern(R"(a(b|c)d)");
    test_pattern(R"(a.*b)");
    test_pattern(R"([abc]d[0-9])");
    test_pattern(R"([abc]\d[0-9])");
    test_pattern(R"(\w+abc)");
    test_pattern(R"(^a.*$)");
    test_pattern(R"((a|b|c)?d)");
    test_pattern(R"(ab|cd.e)");
    test_pattern(R"(a\[b\].*)");
    test_pattern(R"(.+bc|def)");
    test_pattern(R"((ab|cd){2})");
    test_pattern(R"(a?b+c*)");

    // Additional tests for new escapes
    test_pattern(R"(\D\W\S\b\w+)");  // Various negated and boundary
    test_pattern(R"(\p{L}+)");  // Unicode property (simplified)
    test_pattern(R"(\Qspecial$chars(a|b|c) \E(a|b|c))");  // Quoted literal
    test_pattern(R"(a\Ab\Z$)");  // Anchors

    // Tests for lazy/possessive quants
    test_pattern(R"(a*?b)");  // Lazy *
    test_pattern(R"((a{2,5}+|[A-Z]+)c)");  // Possessive {n,m}+

    // Tests for tail handling (invalid/unconsumed)
    test_pattern(R"(a.*b%)");  // % after valid, should be non-lit tail
    test_pattern(R"(a\)");  // Trailing escape, tail "\" as non-lit
    test_pattern(R"({invalid}abc)");  // Invalid {, ignore quant, abc lit
    test_pattern(R"(a|b|c|extra)");  // Alt with extra after last |
    test_pattern(R"(\Qunclosed)");  // Unclosed Q, content as lit
    test_pattern("abc");
    test_pattern(R"((a|b|(c|d))e(f|g))");

    // New complicated | examples (corrected raw strings)
    test_pattern(R"((a|b|(c|d))e(f|g))");
    test_pattern(R"(^(ab|cd){2,4}|(ef|gh)\d+.*$)");
    test_pattern(R"(((a|b|c)?\?|(d|e){1,3}??|[f-g]+)[\w\s]*|(h|i|j)k)");
    test_pattern(R"(\Qabc|def\E(ghi|jkl)mno|\p{Lu}{2,}(pqr|stu))");
    test_pattern(R"((\b(foo|bar)\b|\B(baz|qux)\B){0,2}|(img|vid):(\w+)(alt|src))");
    test_pattern(R"((a{2,5}+(b|c|d)+|(e|f){3,}?g+)+|(h|i)j{1,3}+k)");
    test_pattern(R"((?=(a|b|c))d|e(?<!f|g)h|(i|j|k)l{2}m)");
}
} // extern "C"