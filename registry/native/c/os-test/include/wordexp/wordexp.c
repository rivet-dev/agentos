#include <wordexp.h>
#ifdef wordexp
#undef wordexp
#endif
int (*foo)(const char *restrict, wordexp_t *restrict, int) = wordexp;
int main(void) { return 0; }
