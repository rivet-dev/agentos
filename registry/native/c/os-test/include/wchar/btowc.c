#include <wchar.h>
#ifdef btowc
#undef btowc
#endif
wint_t (*foo)(int) = btowc;
int main(void) { return 0; }
