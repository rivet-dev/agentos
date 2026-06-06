#include <wchar.h>
#ifdef wctob
#undef wctob
#endif
int (*foo)(wint_t) = wctob;
int main(void) { return 0; }
