// Header: ast/regex_literal_splitting.hpp
#pragma once
#include <vector>
#include <string>

struct Segment {
    std::string str;
    bool is_lit = false;
};

std::vector<std::vector<Segment>> regex_literal_splitting(const std::string& p_pattern);

// C-compatible declaration for test()
extern "C" void regex_literal_splitting_test(void);