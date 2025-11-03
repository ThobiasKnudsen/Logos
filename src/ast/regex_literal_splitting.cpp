#include "ast/regex_literal_splitting.hpp"
#include <cctype>
#include <limits>
#include <utility>
#include <iterator>
#include <cstdio> // for printf
#include <string>
#include <vector>
#include <algorithm> // for std::min
#define PCRE2_CODE_UNIT_WIDTH 8
#include <pcre2.h> // For opt-in validation.
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
                if (!min_s.empty()) min_n = std::stoul(min_s);
                size_t max_n = INF;
                if (!max_s.empty()) max_n = std::stoul(max_s);
                return {min_n, max_n};
            }
        } catch (...) {
            return {0, 0}; // Invalid quant.
        }
    }
    return {0, 0}; // Invalid.
}
void merge_adjacent_segments(std::vector<Segment>& path) {
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
// Temp compile for validation.
bool validate_segment(const std::string& seg_str, bool is_lit) {
    if (is_lit) return true;
    std::string pat = "^(" + seg_str + ")";
    int errcode;
    PCRE2_SIZE erroffset;
    pcre2_code* code = pcre2_compile(reinterpret_cast<PCRE2_SPTR>(pat.c_str()),
                                     static_cast<PCRE2_SIZE>(pat.length()),
                                     PCRE2_UTF | PCRE2_UCP,
                                     &errcode, &erroffset, NULL);
    bool valid = (code != nullptr);
    if (!valid) {
        PCRE2_UCHAR errbuf[256];
        pcre2_get_error_message(errcode, errbuf, sizeof(errbuf));
        fprintf(stderr, "Invalid seg '%s': %s at %zu\n", seg_str.c_str(), (char*)errbuf, erroffset);
    }
    if (code) pcre2_code_free(code);
    return valid;
}
// Forward declarations.
std::vector<std::vector<Segment>> parseRE(const std::string& s, size_t& pos);
std::vector<std::vector<Segment>> parseAlt(const std::string& s, size_t& pos);
std::vector<std::vector<Segment>> parseConcat(const std::string& s, size_t& pos);
std::vector<std::vector<Segment>> parseTerm(const std::string& s, size_t& pos);
std::vector<std::vector<Segment>> parseAtom(const std::string& s, size_t& pos);
std::vector<std::vector<Segment>> parseAtom(const std::string& s, size_t& pos) {
    if (pos >= s.size()) return {};
    char c = s[pos];
    if (c == '\\') {
        ++pos;
        if (pos >= s.size()) {
            Segment seg{"\\", true};
            return {{seg}};
        }
        char esc = s[pos++];
        if (esc == 'd' || esc == 'D' || esc == 'w' || esc == 'W' || esc == 's' || esc == 'S' ||
            esc == 'b' || esc == 'B' || esc == 'A' || esc == 'Z' || esc == 'z' || esc == 'R') {
            std::string esc_str = "\\" + std::string(1, esc);
            Segment seg{std::move(esc_str), false};
            return {{seg}};
        } else if (esc == 'p' || esc == 'P') {
            // Fix: Bounds check for underflow.
            if (pos < 2) {
                Segment seg{std::string(1, esc), true};
                return {{seg}};
            }
            size_t start = pos - 2;
            while (pos < s.size() && s[pos] != '}') ++pos;
            if (pos < s.size()) ++pos;
            std::string prop_esc = s.substr(start, pos - start);
            Segment seg{std::move(prop_esc), false};
            return {{seg}};
        } else if (esc == 'Q') {
            size_t q_start = pos;
            while (pos + 1 < s.size()) {
                if (s[pos] == '\\' && s[pos + 1] == 'E') {
                    std::string quoted = s.substr(q_start, pos - q_start);
                    Segment seg{std::move(quoted), true};
                    pos += 2;
                    return {{seg}};
                }
                ++pos;
            }
            std::string quoted = s.substr(q_start, s.size() - q_start);
            Segment seg{std::move(quoted), true};
            pos = s.size();
            return {{seg}};
        } else {
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
        bool empty_class = true;
        while (pos < s.size() && s[pos] != ']') {
            empty_class = false;
            if (s[pos] == '\\' && pos + 1 < s.size()) ++pos;
            if (pos < s.size()) ++pos;
        }
        if (pos < s.size() && s[pos] == ']') ++pos;
        std::string class_str = s.substr(start, pos - start);
        if (empty_class) class_str = "[]";
        Segment seg{std::move(class_str), false};
        return {{seg}};
    } else if (c == '(') {
        ++pos;
        bool is_look = false;
        size_t look_pos = pos; // Fallback.
        if (pos + 1 < s.size() && s[pos] == '?') {
            ++pos;
            if (pos >= s.size()) {
                if (pos > 0) --pos;
                goto standard_group;
            }
            char next = s[pos];
            if (next == '=' || next == '!' || next == ':') {
                is_look = true;
            } else {
                if (pos > 0) --pos;
                goto standard_group;
            }
        }
        if (is_look) {
            // Fix: Bounds + nested paren count.
            if (pos < 2) {
                pos = look_pos;
                goto standard_group;
            }
            size_t group_start = pos - 2; // To ( from next.
            int paren_level = 1; // After (?=
            while (pos < s.size()) {
                if (s[pos] == '(') {
                    ++paren_level;
                } else if (s[pos] == ')') {
                    --paren_level;
                    if (paren_level == 0) {
                        break;
                    }
                }
                ++pos;
            }
            size_t length = pos - group_start;
            if (pos < s.size() && s[pos] == ')') {
                length += 1;
                ++pos;
            } else {
                // Unbalanced: to end.
                pos = s.size();
            }
            std::string look_str = s.substr(group_start, length);
            Segment seg{std::move(look_str), false};
            return {{seg}};
        }
    standard_group:
        // Standard group.
        std::vector<std::vector<Segment>> paths = parseRE(s, pos);
        if (pos < s.size() && s[pos] == ')') ++pos;
        else {
            // Unclosed: tail as non-lit.
            std::string tail = s.substr(pos);
            for (auto& path : paths) {
                path.emplace_back(std::move(tail), false);
            }
        }
        return paths;
    } else if (c == '^' || c == '$') {
        ++pos;
        Segment seg{std::string(1, c), false};
        return {{seg}};
    } else {
        ++pos;
        Segment seg{std::string(1, c), true};
        return {{seg}};
    }
    return {}; // Unreachable.
}
std::vector<std::vector<Segment>> parseTerm(const std::string& s, size_t& pos) {
    auto paths = parseAtom(s, pos);
    if (paths.empty()) return {};
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
            while (pos < s.size() && std::isdigit(static_cast<unsigned char>(s[pos]))) ++pos;
            if (pos < s.size() && s[pos] == ',') ++pos;
            while (pos < s.size() && std::isdigit(static_cast<unsigned char>(s[pos]))) ++pos;
            if (pos < s.size() && s[pos] == '}') {
                ++pos;
                quant_str = s.substr(brace_start, pos - brace_start);
                has_quant = true;
            } else {
                pos = brace_start;
            }
        }
    }
    std::string full_quant = quant_str;
    bool is_lazy = false, is_possessive = false;
    if (has_quant && pos < s.size()) {
        if (s[pos] == '?') {
            full_quant += '?'; is_lazy = true; ++pos;
        } else if (s[pos] == '+') {
            full_quant += '+'; is_possessive = true; ++pos;
        }
    }
    if (has_quant) {
        auto [min_n, max_n] = get_quant_n(quant_str);
        if (min_n == 0 && max_n == 0) {
            // Invalid: tail as non-lit.
            std::string tail = s.substr(pos - full_quant.length());
            for (auto& p : paths) p.emplace_back(std::move(tail), false);
            return paths;
        }
        // Fixed rep: expand (capped).
        if (max_n != std::numeric_limits<size_t>::max() && max_n == min_n && min_n > 0 && min_n < 10) {
            size_t n = min_n;
            std::vector<std::vector<Segment>> repeated{{}};
            for (size_t k = 0; k < n; ++k) {
                std::vector<std::vector<Segment>> new_rep;
                for (const auto& pre : repeated) {
                    for (const auto& p : paths) {
                        std::vector<Segment> np = pre;
                        np.insert(np.end(), p.begin(), p.end());
                        merge_adjacent_segments(np);
                        new_rep.push_back(std::move(np));
                    }
                }
                repeated = std::move(new_rep);
            }
            return repeated;
        }
        // Variable: append to last.
        std::vector<std::vector<Segment>> new_paths;
        for (const auto& p : paths) {
            std::vector<Segment> new_p = p;
            if (!new_p.empty()) {
                new_p.back().str += full_quant;
                new_p.back().is_lit = false;
            } else {
                new_p.emplace_back(full_quant, false);
            }
            merge_adjacent_segments(new_p);
            new_paths.push_back(std::move(new_p));
        }
        if (min_n == 0) new_paths.insert(new_paths.begin(), {});
        return new_paths;
    }
    return paths;
}
std::vector<std::vector<Segment>> parseConcat(const std::string& s, size_t& pos) {
    std::vector<std::vector<std::vector<Segment>>> sub_groups;
    while (pos < s.size() && s[pos] != '|' && s[pos] != ')') {
        auto sub = parseTerm(s, pos);
        if (!sub.empty()) sub_groups.push_back(std::move(sub));
        else break;
    }
    std::vector<std::vector<Segment>> current{{}};
    for (const auto& group : sub_groups) {
        std::vector<std::vector<Segment>> new_current;
        for (const auto& prefix : current) {
            for (const auto& suffix_group : group) {
                std::vector<Segment> new_path = prefix;
                new_path.insert(new_path.end(), suffix_group.begin(), suffix_group.end());
                merge_adjacent_segments(new_path);
                new_current.push_back(std::move(new_path));
            }
        }
        current = std::move(new_current);
    }
    return current;
}
std::vector<std::vector<Segment>> parseAlt(const std::string& s, size_t& pos) {
    auto paths = parseConcat(s, pos);
    while (pos < s.size() && s[pos] == '|') {
        ++pos;
        auto sub = parseConcat(s, pos);
        paths.insert(paths.end(), std::make_move_iterator(sub.begin()), std::make_move_iterator(sub.end()));
    }
    if (pos < s.size() && s[pos] != ')') {
        std::string tail = s.substr(pos);
        for (auto& path : paths) path.emplace_back(std::move(tail), false);
    }
    return paths;
}
std::vector<std::vector<Segment>> parseRE(const std::string& s, size_t& pos) {
    return parseAlt(s, pos);
}
} // namespace
std::vector<std::vector<Segment>> regex_literal_splitting(const std::string& p_pattern, bool validate) {
    auto paths = regex_literal_splitting(p_pattern);
    if (!validate) return paths;
    for (auto& path : paths) {
        for (auto& seg : path) {
            if (!seg.is_lit && !validate_segment(seg.str, false)) {
                seg.str.clear(); // Skip invalid.
            }
        }
    }
    return paths;
}
std::vector<std::vector<Segment>> regex_literal_splitting(const std::string& p_pattern) {
    std::string str = p_pattern;
    size_t pos = 0;
    auto paths = parseRE(str, pos);
    // Fix: Safeguard pos.
    pos = std::min(pos, str.size());
    if (pos < str.size()) {
        std::string tail = str.substr(pos);
        if (paths.empty()) {
            paths.emplace_back();
            paths.back().emplace_back(std::move(tail), false);
        } else {
            for (auto& path : paths) {
                path.emplace_back(std::move(tail), false);
            }
        }
    }
    return paths;
}
void test_pattern(const std::string& pattern, bool do_validate = false) {
    auto paths = regex_literal_splitting(pattern, do_validate);
    printf("%s%s\n", pattern.c_str(), do_validate ? " (validated)" : "");
    bool all_valid = !paths.empty();
    for (const auto& path : paths) {
        if (path.empty()) continue;
        printf("    ");
        for (const auto& seg : path) {
            printf("%s%s ", seg.str.c_str(), seg.is_lit ? " (lit)" : " (regex)");
        }
        printf("\n");
        if (!do_validate) continue;
        bool path_valid = true;
        for (const auto& seg : path) {
            if (!seg.is_lit && seg.str.empty()) path_valid = false;
        }
        if (!path_valid) all_valid = false;
    }
    printf("    %s\n", all_valid ? "VALID" : "INVALID (some segs skipped)");
}
extern "C" {
void regex_literal_splitting_test(void) {
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
    test_pattern(R"(\D\W\S\b\w+)");
    test_pattern(R"(\p{L}+)");
    test_pattern(R"(\Qspecial$chars(a|b|c) \E(a|b|c))");
    test_pattern(R"(a\Ab\Z$)");
    test_pattern(R"(a*?b)");
    test_pattern(R"((a{2,5}+|[A-Z]+)c)");
    test_pattern(R"(a.*b%)");
    test_pattern(R"(a\))");
    test_pattern(R"({invalid}abc)");
    test_pattern(R"(a|b|c|extra)");
    test_pattern(R"(\Qunclosed)");
    test_pattern(R"((a|b|(c|d))e(f|g))");
    test_pattern(R"((a|b|(c|d))e(f|g))");
    test_pattern(R"(^(ab|cd){2,4}|(ef|gh)\d+.*$)");
    test_pattern(R"(((a|b|c)?\?|(d|e){1,3}??|[f-g]+)[\w\s]*|(h|i|j)k)");
    test_pattern(R"(\Qabc|def\E(ghi|jkl)mno|\p{Lu}{2,}(pqr|stu))");
    test_pattern(R"((\b(foo|bar)\b|\B(baz|qux)\B){0,2}|(img|vid):(\w+)(alt|src))");
    test_pattern(R"((a{2,5}+(b|c|d)+|(e|f){3,}?g+)+|(h|i)j{1,3}+k)");
    test_pattern(R"((?=(a|b|c))d|e(?<!f|g)h|(i|j|k)l{2}m)");

    printf("\n--- Validation Mode ---\n");
    test_pattern(R"(a\)", true);
    test_pattern(R"({invalid}abc)", true);
    test_pattern(R"(\Qunclosed)", true);
    test_pattern(R"((?=(a|b|c))d|e(?<!f|g)h|(i|j|k)l{2}m)", true);
}
} // extern "C"