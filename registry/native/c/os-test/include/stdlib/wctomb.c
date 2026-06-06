#include <stdlib.h>
#ifdef wctomb
#undef wctomb
#endif
int (*foo)(char *, wchar_t) = wctomb;
int main(void) { return 0; }
