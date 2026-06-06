#include <wchar.h>
#ifdef wcsrchr
#undef wcsrchr
#endif
wchar_t *(*foo)(const wchar_t *, wchar_t) = wcsrchr;
int main(void) { return 0; }
