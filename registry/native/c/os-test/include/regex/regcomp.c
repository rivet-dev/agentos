#include <regex.h>
#ifdef regcomp
#undef regcomp
#endif
int (*foo)(regex_t *restrict, const char *restrict, int) = regcomp;
int main(void) { return 0; }
