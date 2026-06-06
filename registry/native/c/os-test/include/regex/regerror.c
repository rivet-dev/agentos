#include <regex.h>
#ifdef regerror
#undef regerror
#endif
size_t (*foo)(int, const regex_t *restrict, char *restrict, size_t) = regerror;
int main(void) { return 0; }
