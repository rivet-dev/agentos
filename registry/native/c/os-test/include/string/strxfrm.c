#include <string.h>
#ifdef strxfrm
#undef strxfrm
#endif
size_t (*foo)(char *restrict, const char *restrict, size_t) = strxfrm;
int main(void) { return 0; }
