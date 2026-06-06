#include <stdlib.h>
#ifdef strtoull
#undef strtoull
#endif
unsigned long long (*foo)(const char *restrict, char **restrict, int) = strtoull;
int main(void) { return 0; }
