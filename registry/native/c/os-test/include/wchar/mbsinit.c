#include <wchar.h>
#ifdef mbsinit
#undef mbsinit
#endif
int (*foo)(const mbstate_t *) = mbsinit;
int main(void) { return 0; }
