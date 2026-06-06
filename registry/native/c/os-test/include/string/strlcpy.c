#include <string.h>
#ifdef strlcpy
#undef strlcpy
#endif
size_t (*foo)(char *restrict, const char *restrict, size_t) = strlcpy;
int main(void) { return 0; }
