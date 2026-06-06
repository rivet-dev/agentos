#include <string.h>
#ifdef strncpy
#undef strncpy
#endif
char *(*foo)(char *restrict, const char *restrict, size_t) = strncpy;
int main(void) { return 0; }
