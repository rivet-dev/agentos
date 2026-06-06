#include <string.h>
#ifdef strxfrm_l
#undef strxfrm_l
#endif
size_t (*foo)(char *restrict, const char *restrict, size_t, locale_t) = strxfrm_l;
int main(void) { return 0; }
