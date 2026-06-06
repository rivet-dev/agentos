#include <string.h>
#ifdef stpncpy
#undef stpncpy
#endif
char *(*foo)(char *restrict, const char *restrict, size_t) = stpncpy;
int main(void) { return 0; }
