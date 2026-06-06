#include <string.h>
#ifdef strncat
#undef strncat
#endif
char *(*foo)(char *restrict, const char *restrict, size_t) = strncat;
int main(void) { return 0; }
