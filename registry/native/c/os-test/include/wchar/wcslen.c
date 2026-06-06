#include <wchar.h>
#ifdef wcslen
#undef wcslen
#endif
size_t (*foo)(const wchar_t *) = wcslen;
int main(void) { return 0; }
