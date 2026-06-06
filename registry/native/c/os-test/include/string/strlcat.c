#include <string.h>
#ifdef strlcat
#undef strlcat
#endif
size_t (*foo)(char *restrict, const char *restrict, size_t) = strlcat;
int main(void) { return 0; }
