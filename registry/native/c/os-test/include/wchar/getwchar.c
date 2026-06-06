#include <wchar.h>
#ifdef getwchar
#undef getwchar
#endif
wint_t (*foo)(void) = getwchar;
int main(void) { return 0; }
