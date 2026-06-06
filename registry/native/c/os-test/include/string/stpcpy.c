#include <string.h>
#ifdef stpcpy
#undef stpcpy
#endif
char *(*foo)(char *restrict, const char *restrict) = stpcpy;
int main(void) { return 0; }
