#include <regex.h>
#ifdef regexec
#undef regexec
#endif
int (*foo)(const regex_t *restrict, const char *restrict, size_t, regmatch_t [restrict], int) = regexec;
int main(void) { return 0; }
